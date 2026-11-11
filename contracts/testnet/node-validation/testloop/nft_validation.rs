//! Runs inside the pinned nearcore test-only crate, with simulated transport/time.
//! No HTTP client or existing signer/key-file loader is used by this adapter.
use crate::setup::builder::TestLoopBuilder;
use crate::utils::account::{create_validators_spec, validators_spec_clients};
use crate::utils::network::chunk_endorsement_dropper_by_hash;
use near_async::time::Duration;
use near_chain_configs::Genesis;
use near_primitives::hash::CryptoHash;
use near_primitives::shard_layout::ShardLayout;
use near_primitives::state_record::StateRecord;
use near_primitives::types::{AccountId, Balance, Gas};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

const ACCOUNTS: [&str; 3] = [
    "local-nft-contract.testnet",
    "local-market-contract.testnet",
    "local-market-operator.testnet",
];

fn emit(value: Value) {
    println!("NFT_VALIDATION {}", value);
    io::stdout().flush().unwrap();
}

#[test]
fn bridge() {
    assert!(Path::new("/run/nft-validation-isolated").is_file());
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let init: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    let chain = init["chain_id"].as_str().unwrap();
    assert!(chain.starts_with("nft-validation-local-"));
    assert_eq!(init["public_keys"].as_object().unwrap().len(), 3);
    let layout = ShardLayout::multi_shard_custom(vec!["local-n".parse().unwrap()], 0);
    let nft: AccountId = ACCOUNTS[0].parse().unwrap();
    let market: AccountId = ACCOUNTS[1].parse().unwrap();
    let nft_shard = layout.account_id_to_shard_id(&nft);
    let market_shard = layout.account_id_to_shard_id(&market);
    assert_ne!(nft_shard, market_shard);
    let spec = create_validators_spec(4, 0);
    let clients = validators_spec_clients(&spec);
    let mut builder = TestLoopBuilder::new_genesis_builder()
        .chain_id(chain.to_owned())
        .protocol_version(88)
        .genesis_height(0)
        .validators_spec(spec)
        .shard_layout(layout.clone())
        .gas_limit(Gas::from_gas(1_000_000_000_000_000))
        .gas_prices(
            Balance::from_yoctonear(5000),
            Balance::from_yoctonear(10_000_000_000_000_000_000_000),
        );
    for name in ACCOUNTS {
        assert!(
            init["public_keys"][name]
                .as_str()
                .unwrap()
                .starts_with("ed25519:")
        );
        builder = builder.add_user_account_simple(name.parse().unwrap(), Balance::from_near(10));
    }
    let genesis = builder.build();
    let mut records = Vec::new();
    genesis.for_each_record(|record| {
        let mut record = record.clone();
        if let StateRecord::AccessKey {
            account_id,
            public_key,
            ..
        } = &mut record
        {
            if let Some(key) = init["public_keys"].get(account_id.as_str()) {
                *public_key = key.as_str().unwrap().parse().unwrap();
            }
        }
        records.push(record);
    });
    let genesis = Genesis::new(genesis.config.clone(), records.into()).unwrap();
    let genesis_public = serde_json::to_value(&genesis.config).unwrap();
    let mut env = TestLoopBuilder::new()
        .genesis(genesis)
        .clients(clients)
        .enable_rpc()
        .track_all_shards()
        .build();
    let delayed = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicU64::new(0));
    for node in &env.node_datas {
        let epoch_manager = env
            .test_loop
            .data
            .get(&node.client_sender.actor_handle())
            .client
            .epoch_manager
            .clone();
        let gate = delayed.clone();
        let observed = dropped.clone();
        node.register_override_handler(
            &mut env.test_loop.data,
            chunk_endorsement_dropper_by_hash(
                env.shared_state.chunks_storage.clone(),
                epoch_manager,
                Box::new(move |header| {
                    let withhold = gate.load(Ordering::SeqCst) && header.shard_id() == nft_shard;
                    if withhold {
                        observed.fetch_add(1, Ordering::SeqCst);
                    }
                    withhold
                }),
            ),
        );
    }
    emit(
        json!({"ready":true,"evidence_class":"testloop-simulated-network","source_commit":"b7b4737cab46f17a6daa2ed694d7aebbd204363a",
                "validators":4,"nft_shard":nft_shard,"marketplace_shard":market_shard,"genesis_config":genesis_public,
                "control":"withhold genuine NFT-shard chunk endorsements; never edit receipts/results"}),
    );
    for line in lines {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        match request["op"].as_str().unwrap() {
            "stop" => break,
            "advance" => {
                let seconds = request["seconds"].as_i64().unwrap();
                assert!((0..=600).contains(&seconds));
                env.test_loop.run_for(Duration::seconds(seconds));
                emit(
                    json!({"advanced_seconds":seconds,"withheld_endorsements":dropped.load(Ordering::SeqCst)}),
                );
            }
            "wait_parent_and_delay" => {
                let hash: CryptoHash = request["hash"].as_str().unwrap().parse().unwrap();
                let tx = env
                    .rpc_runner()
                    .run_until_outcome_available(hash, Duration::seconds(30));
                let root_id = tx.outcome_with_id.outcome.receipt_ids[0];
                let parent = env
                    .rpc_runner()
                    .run_until_outcome_available(root_id, Duration::seconds(30));
                delayed.store(true, Ordering::SeqCst);
                emit(
                    json!({"root_receipt":root_id,"committed_parent_block":parent.block_hash,
                            "child_receipts":parent.outcome_with_id.outcome.receipt_ids,"nft_shard_delayed":true}),
                );
            }
            "resume" => {
                delayed.store(false, Ordering::SeqCst);
                emit(
                    json!({"nft_shard_delayed":false,"withheld_endorsements":dropped.load(Ordering::SeqCst)}),
                );
            }
            "rpc" => {
                let rpc = &request["request"];
                let method = rpc["method"].as_str().unwrap();
                assert!(
                    [
                        "status",
                        "block",
                        "chunk",
                        "query",
                        "EXPERIMENTAL_protocol_config",
                        "EXPERIMENTAL_receipt",
                        "EXPERIMENTAL_tx_status",
                        "send_tx"
                    ]
                    .contains(&method)
                );
                if method == "query" {
                    assert!(ACCOUNTS.contains(&rpc["params"]["account_id"].as_str().unwrap()));
                }
                let client = env.node_datas[env.rpc_data_idx()].jsonrpc_client();
                assert!(
                    client.server_addr.is_none(),
                    "external transport prohibited"
                );
                // run_future owns a 'static future; do not borrow a local client.
                let transport = client.transport.clone();
                let payload = serde_json::to_vec(rpc).unwrap();
                let future = async move {
                    transport
                        .send_http_request("/", payload, 32 * 1024 * 1024, &[])
                        .await
                };
                let (status, bytes) = env
                    .rpc_runner()
                    .run_future("validation_rpc", future, Duration::seconds(30))
                    .unwrap();
                assert_eq!(status.as_u16(), 200);
                emit(serde_json::from_slice(&bytes).unwrap());
            }
            // Receipt payload splitting would invalidate a shared chunk proof.
            // Capability is explicit: the caller must classify this as BLOCKED,
            // never replace an authentic dependency with a fabricated result.
            "individual_data_delivery" => emit(
                json!({"blocked":"individual DataReceipts inside one authenticated chunk proof cannot be independently scheduled by this transport adapter"}),
            ),
            _ => panic!("unknown local adapter operation"),
        }
    }
}
