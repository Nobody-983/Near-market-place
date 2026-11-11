//! Read only the stopped database; never load nearcore config, node or wallet keys.
//! Installed as core/store/src/bin/nft-receipt-exporter.rs at the pinned commit.
use anyhow::{Result, anyhow, ensure};
use borsh::BorshDeserialize;
use near_primitives::hash::{CryptoHash, hash};
use near_primitives::receipt::{ProcessedReceiptMetadata, Receipt};
use near_primitives::views::ReceiptView;
use near_store::{DBCol, Mode, NodeStorage, StoreConfig};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::Path;

const COMMIT: &str = "b7b4737cab46f17a6daa2ed694d7aebbd204363a";

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && args[1] == "--version" {
        println!("nft-receipt-exporter nearcore 2.14.0-rc.5 {COMMIT} schema 1");
        return Ok(());
    }
    ensure!(
        args.len() == 1,
        "no path or key arguments accepted; use the isolated Python launcher"
    );
    ensure!(
        Path::new("/run/nft-validation-isolated").is_file(),
        "isolated launcher required"
    );
    ensure!(
        Path::new("/db/data/CURRENT").is_file(),
        "stopped hot database missing"
    );
    let hot = StoreConfig {
        path: Some("data".into()),
        ..StoreConfig::default()
    };
    let cold = Path::new("/db/cold-data/CURRENT")
        .is_file()
        .then(|| StoreConfig {
            path: Some("cold-data".into()),
            ..StoreConfig::default()
        });
    let storage = NodeStorage::opener(Path::new("/db"), &hot, cold.as_ref(), None)
        .open_in_mode(Mode::ReadOnly)?;
    let store = storage
        .get_split_store()
        .unwrap_or_else(|| storage.get_hot_store());
    let identity: Value = serde_json::from_slice(&fs::read("/opt/identity.json")?)?;
    let height = identity["genesis_height"]
        .as_u64()
        .ok_or_else(|| anyhow!("missing genesis height"))?;
    let genesis: CryptoHash = store
        .get_ser(DBCol::BlockHeight, &height.to_le_bytes())
        .ok_or_else(|| anyhow!("genesis height absent from stopped store"))?;
    ensure!(
        identity["genesis_hash"].as_str() == Some(genesis.to_string().as_str()),
        "stopped store genesis mismatch"
    );
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    writeln!(
        out,
        "{}",
        json!({"kind":"header", "schema":1, "source_commit":COMMIT,
        "mode":"read-only", "columns":["Receipts", "ProcessedReceiptIds"],
        "genesis_height":height,"genesis_block_hash":genesis})
    )?;
    let mut counts = [0_u64; 2];
    for (key, value) in store.iter(DBCol::Receipts) {
        let receipt = Receipt::try_from_slice(&value)?;
        ensure!(
            key.as_ref() == receipt.receipt_id().as_ref(),
            "receipt database key mismatch"
        );
        writeln!(
            out,
            "{}",
            json!({"kind":"receipt", "column":"Receipts",
            "key_hex":hex::encode(&key), "borsh_hex":hex::encode(&value),
            "sha256":hex::encode(hash(&value).0), "receipt":ReceiptView::from(receipt)})
        )?;
        counts[0] += 1;
    }
    for (key, value) in store.iter(DBCol::ProcessedReceiptIds) {
        let rows = Vec::<ProcessedReceiptMetadata>::try_from_slice(&value)?;
        ensure!(key.len() == 40, "processed metadata key size");
        let metadata: Vec<_> = rows
            .iter()
            .map(|r| {
                json!({
                    "receipt_id":r.receipt_id(), "source":format!("{:?}",r.source())
                })
            })
            .collect();
        writeln!(
            out,
            "{}",
            json!({"kind":"processed", "column":"ProcessedReceiptIds",
            "key_hex":hex::encode(&key), "borsh_hex":hex::encode(&value),
            "sha256":hex::encode(hash(&value).0), "metadata":metadata})
        )?;
        counts[1] += 1;
    }
    writeln!(
        out,
        "{}",
        json!({"kind":"end", "receipts":counts[0], "processed_rows":counts[1]})
    )?;
    out.flush()?;
    Ok(())
}
