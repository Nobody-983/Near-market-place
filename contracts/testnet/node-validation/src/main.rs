//! Ephemeral signers. No filesystem/credential reads and no network client.
use base64::{Engine, engine::general_purpose::STANDARD};
use near_crypto::{InMemorySigner, KeyType, SecretKey, Signer};
use near_primitives::{
    hash::{CryptoHash, hash},
    transaction::{
        Action, DeployContractAction, FunctionCallAction, SignedTransaction, Transaction,
        TransactionV0,
    },
    types::{Balance, Gas},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
};

const NFT: &str = "local-nft-contract.testnet";
const MARKET: &str = "local-market-contract.testnet";
const OWNER: &str = "local-market-operator.testnet";
const CAP: u64 = 300_000_000_000_000;
const NFT_HASH: &str = "b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d";
const MARKET_HASH: &str = "5f0b522306a87bc01beebe4c7575b8ae705785d38f003feae18eebfe2835708a";

fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("missing string {key}"))
}
fn actions(v: &Value, signer: &str, receiver: &str) -> Result<Vec<Action>, String> {
    if ![NFT, MARKET, OWNER].contains(&signer) || ![NFT, MARKET].contains(&receiver) {
        return Err("local account allowlist".into());
    }
    let rows = v.as_array().ok_or("actions must be array")?;
    if rows.is_empty() || rows.len() > 3 {
        return Err("action count".into());
    }
    let mut out = Vec::new();
    let mut gas_sum = 0u64;
    for (i, row) in rows.iter().enumerate() {
        match required(row, "kind")? {
            "deploy" => {
                if i != 0 || signer != receiver || rows.len() < 2 {
                    return Err("atomic self deployment required".into());
                }
                let bytes = STANDARD
                    .decode(required(row, "code")?)
                    .map_err(|_| "invalid code")?;
                let digest = hash(&bytes)
                    .0
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                if digest
                    != if receiver == NFT {
                        NFT_HASH
                    } else {
                        MARKET_HASH
                    }
                {
                    return Err("unapproved Wasm".into());
                }
                out.push(Action::DeployContract(DeployContractAction { code: bytes }));
            }
            "call" => {
                let method = required(row, "method")?;
                let init = method == "new_testnet" || method == "bootstrap_with_admission_owner";
                if init {
                    if signer != receiver || i == 0 || required(&rows[0], "kind")? != "deploy" {
                        return Err("initializer must follow self deploy".into());
                    }
                } else if signer != OWNER {
                    return Err("only disposable owner may call".into());
                }
                let allowed = if receiver == NFT {
                    ["new_testnet", "nft_mint", "nft_approve", "nft_revoke"].as_slice()
                } else {
                    [
                        "bootstrap_with_admission_owner",
                        "fund_testnet_storage",
                        "create_nft_admission",
                        "dispatch_nft_admission",
                        "release_nft_admission",
                    ]
                    .as_slice()
                };
                if !allowed.contains(&method) {
                    return Err("method not allowed".into());
                }
                let args = &row["args"];
                if !args.is_object() {
                    return Err("args must be object".into());
                }
                if method == "nft_approve"
                    && (args.get("msg") != Some(&Value::Null) || args["account_id"] != MARKET)
                {
                    return Err("public approval intake prohibited".into());
                }
                if method == "nft_mint" && args["token_owner_id"] != OWNER {
                    return Err("self mint only".into());
                }
                if method == "nft_revoke" && args["account_id"] != MARKET {
                    return Err("revoke target".into());
                }
                let gas: u64 = required(row, "gas")?.parse().map_err(|_| "gas")?;
                let deposit: u128 = required(row, "deposit")?.parse().map_err(|_| "deposit")?;
                gas_sum = gas_sum.checked_add(gas).ok_or("gas overflow")?;
                if gas == 0 || gas_sum > CAP {
                    return Err("300 Tgas cap".into());
                }
                if (init || receiver == MARKET && method != "fund_testnet_storage") && deposit != 0
                {
                    return Err("zero deposit required".into());
                }
                out.push(Action::FunctionCall(Box::new(FunctionCallAction {
                    method_name: method.into(),
                    args: serde_json::to_vec(args).map_err(|_| "args")?,
                    gas: Gas::from_gas(gas),
                    deposit: Balance::from_yoctonear(deposit),
                })));
            }
            _ => return Err("only deploy/function call actions allowed".into()),
        }
    }
    Ok(out)
}
fn sign(v: &Value, keys: &BTreeMap<String, Signer>) -> Result<Value, String> {
    let signer_id = required(v, "signer")?;
    let receiver_id = required(v, "receiver")?;
    let signer = keys.get(signer_id).ok_or("unknown local signer")?;
    let block_hash: CryptoHash = required(v, "block_hash")?
        .parse()
        .map_err(|_| "block hash")?;
    let nonce: u64 = required(v, "nonce")?.parse().map_err(|_| "nonce")?;
    let tx = Transaction::V0(TransactionV0 {
        signer_id: signer_id.parse().map_err(|_| "signer")?,
        public_key: signer.public_key(),
        nonce,
        receiver_id: receiver_id.parse().map_err(|_| "receiver")?,
        block_hash,
        actions: actions(&v["actions"], signer_id, receiver_id)?,
    });
    let (h, _) = tx.get_hash_and_size();
    let signed = SignedTransaction::new(signer.sign(h.as_ref()), tx);
    Ok(
        json!({"hash": h.to_string(), "signed_tx_base64": STANDARD.encode(borsh::to_vec(&signed).map_err(|_| "borsh")?)}),
    )
}
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--public-receipt-fixtures") {
        public_receipt_fixtures();
        return;
    }
    if std::env::args().nth(1).as_deref() != Some("--disposable-local-only") {
        eprintln!("requires --disposable-local-only; never reads credentials");
        std::process::exit(2);
    }
    let keys: BTreeMap<String, Signer> = [NFT, MARKET, OWNER]
        .into_iter()
        .map(|id| {
            (
                id.into(),
                InMemorySigner::from_secret_key(
                    id.parse().unwrap(),
                    SecretKey::from_random(KeyType::ED25519),
                ),
            )
        })
        .collect();
    println!(
        "{}",
        json!({"public_keys": keys.iter().map(|(id,s)| (id.clone(), s.public_key().to_string())).collect::<BTreeMap<_,_>>()})
    );
    io::stdout().flush().unwrap();
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|_| "input".to_string())
            .and_then(|l| serde_json::from_str(&l).map_err(|_| "json".to_string()))
            .and_then(|v| sign(&v, &keys));
        println!(
            "{}",
            match result {
                Ok(v) => v,
                Err(e) => json!({"error":e}),
            }
        );
        io::stdout().flush().unwrap();
    }
}

/// Public deterministic codec vectors only; no signers, node, or filesystem.
fn public_receipt_fixtures() {
    use near_crypto::PublicKey;
    use near_primitives::receipt::{ActionReceipt, DataReceipt, Receipt, ReceiptEnum, ReceiptV0};
    use near_primitives::transaction::TransferAction;
    use near_primitives::views::ReceiptView;
    let bodies = [
        ReceiptEnum::Action(ActionReceipt {
            signer_id: OWNER.parse().unwrap(),
            signer_public_key: PublicKey::empty(KeyType::ED25519),
            gas_price: Balance::from_yoctonear(5000),
            output_data_receivers: vec![],
            input_data_ids: vec![],
            actions: vec![
                Action::DeployContract(DeployContractAction {
                    code: b"codec-only".to_vec(),
                }),
                Action::FunctionCall(Box::new(FunctionCallAction {
                    method_name: "nft_token".into(),
                    args: br#"{"token_id":"fixture"}"#.to_vec(),
                    gas: Gas::from_gas(4),
                    deposit: Balance::ZERO,
                })),
                Action::Transfer(TransferAction {
                    deposit: Balance::from_yoctonear(2),
                }),
            ],
        }),
        ReceiptEnum::Data(DataReceipt {
            data_id: CryptoHash([4; 32]),
            data: Some(b"true".to_vec()),
        }),
        ReceiptEnum::Data(DataReceipt {
            data_id: CryptoHash([5; 32]),
            data: None,
        }),
    ];
    let receipts: Vec<_> = bodies
        .into_iter()
        .enumerate()
        .map(|(i, body)| {
            let receipt = Receipt::V0(ReceiptV0 {
                predecessor_id: NFT.parse().unwrap(),
                receiver_id: MARKET.parse().unwrap(),
                receipt_id: CryptoHash([i as u8 + 1; 32]),
                receipt: body,
            });
            let raw = borsh::to_vec(&receipt).unwrap();
            json!({"borsh_hex": raw.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "receipt": ReceiptView::from(receipt)})
        })
        .collect();
    println!(
        "{}",
        json!({"evidence_class":"synthetic-codec-fixtures", "receipts":receipts})
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prohibited_accounts_actions_and_intake() {
        let call = json!([{"kind":"call","method":"nft_approve","args":{"account_id":MARKET,"msg":null},"gas":"4","deposit":"1"}]);
        assert!(actions(&call, OWNER, NFT).is_ok());
        assert!(actions(&call, "ahmed-market-operator.testnet", NFT).is_err());
        assert!(actions(&call, OWNER, "ahmed-nft-contract.testnet").is_err());
        for method in [
            "nft_transfer",
            "nft_on_approve",
            "on_nft_verified",
            "purchase",
            "withdraw",
        ] {
            let mut bad = call.clone();
            bad[0]["method"] = json!(method);
            assert!(actions(&bad, OWNER, MARKET).is_err());
        }
        let mut bad = call.clone();
        bad[0]["args"]["msg"] = json!("{}");
        assert!(actions(&bad, OWNER, NFT).is_err());
        let mut bad = call;
        bad[0]["gas"] = json!((CAP + 1).to_string());
        assert!(actions(&bad, OWNER, NFT).is_err());
    }
}
