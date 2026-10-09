// SPDX-License-Identifier: Apache-2.0
//
// Task 3 (oracle side): run the crate verifier (`zkprof_verify::verify`) over
// every case in fixtures/twin-cases and emit the crate's verdict + named checks
// to fixtures/twin-oracle/crate-verdicts.json. The JS twin driver reads this to
// build the agreement table, so both sides run over the IDENTICAL case files.
//
// A case file is:
//   { "kind": "...", "name": "...", "expected_image_id": "sha256:.."|null,
//     "receipt": { seal, imageId, journal } }
//
// Usage: cargo run -p zkprof-verify --bin crate_verdicts
// Exit 0.

use std::path::Path;

// We need a value-level re-serialization of the crate's VerifyResult so the
// twin can read the ORACLE side of the agreement table. Keep only the fields
// the table uses.
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct Verdict {
    kind: String,
    name: String,
    valid: bool,
    journal_digest: Option<String>,
    checks: Vec<NamedCheck>,
}

#[derive(Serialize)]
struct NamedCheck {
    name: String,
    passed: Option<bool>,
    detail: String,
}

fn main() {
    let cases_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/twin-cases");
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/twin-oracle");
    std::fs::create_dir_all(&out_dir).expect("create twin-oracle dir");

    let mut verdicts: Vec<Verdict> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&cases_dir) {
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let case: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("parse case");
            let kind = case
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let name = case
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let expected = case
                .get("expected_image_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let receipt = case.get("receipt").expect("receipt").to_string();

            let result = zkprof_verify::verify(&receipt, expected.as_deref());
            let checks: Vec<NamedCheck> = result
                .checks
                .iter()
                .map(|c| NamedCheck {
                    name: c.name.clone(),
                    passed: c.passed,
                    detail: c.detail.clone(),
                })
                .collect();
            verdicts.push(Verdict {
                kind,
                name,
                valid: result.valid,
                journal_digest: result.journal_digest,
                checks,
            });
        }
    }

    let out_path = out_dir.join("crate-verdicts.json");
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&verdicts).expect("serialize verdicts"),
    )
    .expect("write crate verdicts");
    eprintln!("[crate-verdicts] wrote {}", out_path.display());
    for v in &verdicts {
        eprintln!(
            "[crate-verdicts] {}/{} valid={} journalDigest={:?}",
            v.kind, v.name, v.valid, v.journal_digest
        );
    }
}
