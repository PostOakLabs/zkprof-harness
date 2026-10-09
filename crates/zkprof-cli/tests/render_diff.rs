// SPDX-License-Identifier: Apache-2.0

//! End-to-end behaviour of the `render` and `diff` subcommands.
//!
//! These drive the compiled `zkprof` binary the way a user would, including the
//! part that matters most for W3.6: a document that does not validate against
//! the vendored zkprof-report schema must be rejected rather than rendered.
//!
//! The fixtures are hand-built `zkprof-report` documents. `workload_id` is set
//! to `ci-fixture-not-a-measurement` on purpose: these are test artifacts, not
//! real measurements, and a fixture that pretended otherwise would be a lie in
//! the data.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

/// The compiled binary Cargo makes available to integration tests.
const ZKPROF: &str = env!("CARGO_BIN_EXE_zkprof");

/// A temp file removed when it goes out of scope.
struct TempReport {
    path: PathBuf,
}

impl TempReport {
    /// Writes `value` as JSON into a fresh temp file.
    fn new(value: &Value) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zkprof-cli-test-{}-{}.json", std::process::id(), n));
        let mut file = File::create(&path).expect("a temp report file should be creatable");
        file.write_all(value.to_string().as_bytes())
            .expect("writing the fixture should succeed");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempReport {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A valid, schema-conforming report with two primitive rows.
fn valid_report(workload_id: &str, msm_ms: f64, other_ms: f64) -> Value {
    let total = msm_ms + other_ms;
    serde_json::json!({
        "$schema": "https://postoaklabs.github.io/engineering-decision-schema/zkprof-report.schema.json",
        "report_version": "0.1.0",
        "target": {
            "kind": "native",
            "os": "linux",
            "cpu": "x86_64"
        },
        "system": {
            "proof_system": "groth16",
            "curve_or_field": "bn254"
        },
        "workload_id": workload_id,
        "total_ms": total,
        "phases": [],
        "primitives": [
            {
                "name": "msm",
                "ms": msm_ms,
                "share": msm_ms / total
            },
            {
                "name": "other",
                "ms": other_ms,
                "share": other_ms / total
            }
        ],
        "memory": {},
        "threads": { "count": 1, "shared_array_buffer": false },
        "crossOriginIsolated": false,
        "timer_resolution_us": 0.1,
        "iterations": 1,
        "run_manifest_ref": null
    })
}

/// Runs `zkprof render <path>` and returns (exit success, combined output).
fn run_render(path: &Path) -> (bool, String) {
    let output = Command::new(ZKPROF)
        .arg("render")
        .arg(path)
        .output()
        .expect("the zkprof binary should run");
    let combined = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), combined)
}

/// Runs `zkprof diff <base> <compare>` and returns (exit success, combined output).
fn run_diff(base: &Path, compare: &Path) -> (bool, String) {
    let output = Command::new(ZKPROF)
        .arg("diff")
        .arg(base)
        .arg(compare)
        .output()
        .expect("the zkprof binary should run");
    let combined = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), combined)
}

#[test]
fn render_prints_a_table_for_a_valid_report() {
    let report = TempReport::new(&valid_report("ci-fixture-not-a-measurement", 7.5, 2.5));
    let (ok, out) = run_render(report.path());

    assert!(ok, "render should succeed on a valid report; got:\n{out}");
    assert!(out.contains("zkprof render"), "header line missing: {out}");
    assert!(
        out.contains("ci-fixture-not-a-measurement"),
        "workload id missing: {out}"
    );
    assert!(out.contains("msm"), "msm row missing: {out}");
    assert!(out.contains("other"), "other row missing: {out}");
}

#[test]
fn render_rejects_a_report_with_shares_that_do_not_sum_to_one() {
    // Shares 0.5 and 0.2 sum to 0.7, outside the 0.02 tolerance. The document
    // is well-formed JSON and matches the struct, but `validate` must still
    // reject it and `render` must refuse to print a table.
    let mut bad = valid_report("ci-fixture-not-a-measurement", 7.5, 2.5);
    bad["primitives"][0]["share"] = serde_json::json!(0.5);
    bad["primitives"][1]["share"] = serde_json::json!(0.2);

    let report = TempReport::new(&bad);
    let (ok, out) = run_render(report.path());

    assert!(
        !ok,
        "render must refuse a report whose shares do not partition: {out}"
    );
    assert!(
        out.contains("malformed"),
        "error should call the report malformed: {out}"
    );
}

#[test]
fn render_rejects_a_document_that_is_not_a_report() {
    let not_a_report = serde_json::json!({ "hello": "world" });
    let report = TempReport::new(&not_a_report);
    let (ok, out) = run_render(report.path());

    assert!(!ok, "render must refuse a non-report document: {out}");
    assert!(
        out.contains("not a zkprof-report"),
        "error should say it is not a report: {out}"
    );
}

#[test]
fn diff_prints_a_per_primitive_delta() {
    let base = TempReport::new(&valid_report("ci-fixture-not-a-measurement", 8.0, 2.0));
    let compare = TempReport::new(&valid_report("ci-fixture-not-a-measurement", 4.0, 6.0));
    let (ok, out) = run_diff(base.path(), compare.path());

    assert!(ok, "diff should succeed on two valid reports; got:\n{out}");
    assert!(out.contains("zkprof diff"), "header line missing: {out}");
    // msm went 8.0 -> 4.0: a delta of -4.0 ms and -50%.
    assert!(out.contains("msm"), "msm row missing: {out}");
    assert!(out.contains("other"), "other row missing: {out}");
    // -50% reads as a real delta, not n/a.
    assert!(
        out.contains("-50.0%"),
        "expected a -50% delta on msm: {out}"
    );
}

#[test]
fn diff_rejects_a_malformed_base_report() {
    let mut bad = valid_report("ci-fixture-not-a-measurement", 8.0, 2.0);
    bad["primitives"][0]["share"] = serde_json::json!(0.9);
    bad["primitives"][1]["share"] = serde_json::json!(0.9);
    let base = TempReport::new(&bad);
    let compare = TempReport::new(&valid_report("ci-fixture-not-a-measurement", 4.0, 6.0));

    let (ok, out) = run_diff(base.path(), compare.path());
    assert!(!ok, "diff must refuse a malformed base: {out}");
    assert!(
        out.contains("malformed"),
        "error should call the base malformed: {out}"
    );
}
