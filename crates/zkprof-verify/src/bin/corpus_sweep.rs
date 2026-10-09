// SPDX-License-Identifier: Apache-2.0
//
// Corpus sweep, crate side (corpus-sweep-8). Runs the crate verifier
// (`zkprof_verify::verify`) over EVERY receipt in fixtures/compute-proof and
// writes the crate's verdict (valid + named checks) to
// fixtures/corpus-sweep/crate-verdicts.json. The JS twin driver
// (web/twin/run-corpus-twin.mjs) runs the independent twin over the SAME
// receipts and oracles, and the aggregation produces docs/TWIN-CORPUS-SWEEP.md.
//
// Usage: cargo run -p zkprof-verify --bin corpus_sweep
// Exit 0.

use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct Verdict {
    name: String,
    valid: bool,
    journal_digest: Option<String>,
    checks: Vec<NamedCheck>,
}

#[derive(Serialize)]
struct NamedCheck {
    name: String,
    passed: Option<bool>,
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/compute-proof");
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus-sweep");
    std::fs::create_dir_all(&out_dir).expect("create corpus-sweep dir");

    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("read compute-proof dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().contains(".receipt."))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();

    let mut verdicts: Vec<Verdict> = Vec::new();
    for path in &paths {
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(".receipt.json", "");
        let receipt = std::fs::read_to_string(path).expect("read receipt");
        let result = zkprof_verify::verify(&receipt, None);
        let checks = result
            .checks
            .iter()
            .map(|c| NamedCheck {
                name: c.name.clone(),
                passed: c.passed,
            })
            .collect();
        verdicts.push(Verdict {
            name,
            valid: result.valid,
            journal_digest: result.journal_digest,
            checks,
        });
    }

    let out_path = out_dir.join("crate-verdicts.json");
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&verdicts).expect("serialize"),
    )
    .expect("write");
    eprintln!(
        "[corpus-sweep] crate: {} receipts -> {}",
        verdicts.len(),
        out_path.display()
    );
    let pass = verdicts.iter().filter(|v| v.valid).count();
    eprintln!(
        "[corpus-sweep] crate: {pass} PASS, {} FAIL",
        verdicts.len() - pass
    );
}
