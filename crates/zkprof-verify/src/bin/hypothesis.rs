// SPDX-License-Identifier: Apache-2.0
//
// Journal-bytes hypothesis harness (Task 2). Reads estate ZkVmReceipt JSON
// fixtures, computes the journal-bytes candidates (RFC 8785/JCS primary, plus
// compact and pretty JSON alternates), and attempts a full risc0 3.0.5 Groth16
// verification for each. Prints the per-receipt per-candidate result so the
// finding is empirical, not asserted.
//
// This is the evidence for the estate row RECEIPT-JOURNAL-BYTES-CONTRACT-1: it
// answers "do the guests commit JCS-canonical bytes of the journal JSON object?"
// without the guest source, by actually trying to verify.
//
// Usage: cargo run -p zkprof-verify --bin hypothesis [PATH ...]   (no args -> fixtures/ dir)
// Exit 0.

use std::path::{Path, PathBuf};
use std::time::Instant;

use base64::Engine;
use serde_json::Value;
use zkprof_verify::run_risc0_verify;

fn main() {
    let mut paths: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/compute-proof");
        if let Ok(rd) = std::fs::read_dir(&dir) {
            paths = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
                .collect();
        }
    }
    paths.sort();

    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        let receipt: Value = serde_json::from_str(&text).unwrap();
        let image_id = receipt
            .get("imageId")
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        let journal = receipt.get("journal").unwrap_or(&Value::Null);
        let seal_b64 = receipt.get("seal").and_then(|v| v.as_str());
        let seal =
            match seal_b64.and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok()) {
                Some(b) => b,
                None => {
                    println!(
                        "== {} : NO DECODABLE SEAL ==",
                        path.file_name().unwrap().to_string_lossy()
                    );
                    continue;
                }
            };
        println!(
            "== {} : imageId {}  (seal {} bytes) ==",
            path.file_name().unwrap().to_string_lossy(),
            image_id,
            seal.len()
        );

        // Single source of truth for the candidate set (lies in the crate, so
        // this list cannot drift from the one the digest table is built from).
        let candidates = zkprof_verify::journal_candidate_bytes(journal);
        for (label, bytes) in candidates {
            let start = Instant::now();
            let outcome = run_risc0_verify(&seal, &bytes, Some(image_id));
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let digest = zkprof_verify::journal_digest_hex(&bytes);
            let status = match &outcome {
                Ok(()) => "VERIFIED".to_string(),
                Err(e) => e.clone(),
            };
            println!("    [{label}] digest={digest} -> {status}  ({ms:.1} ms)");
        }
        println!();
    }
}
