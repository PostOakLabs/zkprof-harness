// SPDX-License-Identifier: Apache-2.0

//! The `csp-benchmarks` exporter.
//!
//! The claim under test is not that the JSON has the right shape — that is the
//! easy part — but that the exporter is *honest about its own limits*:
//!
//! 1. It refuses workloads that are not canonical csp circuits, rather than
//!    mapping them to a lookalike. Merkle-Poseidon is not `poseidon`.
//! 2. It omits what it did not measure, rather than emitting a zero that reads
//!    as a measurement of zero.
//! 3. It divides by `iterations`, so the published figure is one proof rather
//!    than the whole measured region.

use std::time::{Duration, UNIX_EPOCH};

use zkprof_core::csp::{
    build_document, build_measurement, detect_target, rfc3339_utc, CspMetadata, ExportError,
    CANONICAL_TARGETS,
};
use zkprof_core::model::{Memory, Report, System, Target, Threads};
use zkprof_core::{REPORT_VERSION, SCHEMA_ID};

/// A report with only the fields the exporter reads set.
///
/// Built as a struct literal rather than through the profiler because these
/// tests are about the conversion, not about measurement.
fn report(workload_id: &str, total_ms: f64, iterations: u32) -> Report {
    Report {
        schema: Some(SCHEMA_ID.to_owned()),
        report_version: REPORT_VERSION.to_owned(),
        target: Target::native(),
        system: System::new("groth16", "bn254").with_constraint_count(15_553),
        workload_id: workload_id.to_owned(),
        total_ms,
        phases: Vec::new(),
        primitives: Vec::new(),
        memory: Memory {
            peak_heap_bytes: None,
        },
        threads: Threads::single(),
        cross_origin_isolated: false,
        timer_resolution_us: 5.0,
        iterations,
        run_manifest_ref: None,
        gpu_timing_source: None,
        backend_id: None,
        version: None,
        commitment_scheme: None,
    }
}

fn metadata() -> CspMetadata {
    CspMetadata::new("2026-08-30T00:00:00Z", "deadbeef")
}

#[test]
fn a_canonical_circuit_in_the_workload_id_is_recognised() {
    assert_eq!(detect_target("sha256-2048"), Some("sha256"));
    assert_eq!(detect_target("csp_keccak_512"), Some("keccak"));
    assert_eq!(detect_target("ECDSA"), Some("ecdsa"));
}

#[test]
fn merkle_poseidon_is_not_mistaken_for_the_poseidon_target() {
    // The load-bearing one. A Merkle tree over Poseidon is a different circuit
    // from "hash N field elements", and csp-benchmarks' `poseidon` target means
    // the latter. Reporting our depth-64 tree as `poseidon, input_size 64`
    // would publish our duration against a workload nobody ran.
    assert_eq!(detect_target("merkle-poseidon-depth64"), None);
    assert_eq!(detect_target("merkle-poseidon-depth-8"), None);
}

#[test]
fn proof_duration_is_one_proof_not_the_whole_measured_region() {
    let report = report("sha256-2048", 2_000.0, 4);
    let row = build_measurement("zkprof", "sha256", &report, Some(2048)).expect("a row");

    // 2000 ms over 4 iterations is 500 ms per proof, which is 500_000 us.
    // Publishing 2_000_000 us would overstate the proof by 4x.
    assert_eq!(row.proof_duration, Some(500_000));
    assert_eq!(row.num_constraints, Some(15_553));
    assert_eq!(row.input_size, Some(2048));
}

#[test]
fn what_was_not_measured_is_absent_rather_than_zero() {
    let report = report("sha256-2048", 1_000.0, 1);
    let row = build_measurement("zkprof", "sha256", &report, None).expect("a row");

    assert_eq!(
        row.verify_duration, None,
        "zkprof verifies outside the region"
    );
    assert_eq!(row.proof_size, None, "zkprof never captures a proof size");
    assert_eq!(row.preprocessing_size, None);

    // The stronger form: the keys must not appear in the JSON at all. A
    // `"proof_size": 0` would survive a schema check and then be read as a
    // measurement of zero.
    let json = serde_json::to_string(&row).expect("serialisable");
    for key in ["verify_duration", "proof_size", "preprocessing_size"] {
        assert!(
            !json.contains(key),
            "{key} must be omitted, not defaulted: {json}"
        );
    }
    assert_eq!(
        row.input_size, None,
        "a fixed-size target has no input size"
    );
}

#[test]
fn peak_memory_is_carried_only_when_it_was_measured() {
    let mut native = report("sha256-2048", 1_000.0, 1);
    assert_eq!(
        build_measurement("zkprof", "sha256", &native, None)
            .expect("a row")
            .peak_memory,
        None,
        "the peak-heap probe returns nothing off wasm"
    );

    native.memory.peak_heap_bytes = Some(23_701_094);
    assert_eq!(
        build_measurement("zkprof", "sha256", &native, None)
            .expect("a row")
            .peak_memory,
        Some(23_701_094)
    );
}

#[test]
fn a_non_canonical_target_is_refused() {
    let report = report("merkle-poseidon-depth64", 1_000.0, 1);
    let error = build_document("zkprof", "merkle-poseidon", metadata(), &[(&report, None)])
        .expect_err("must refuse");
    assert_eq!(
        error,
        ExportError::NotACanonicalTarget {
            target: "merkle-poseidon".to_owned()
        }
    );
}

#[test]
fn a_report_with_no_measured_time_is_refused() {
    let report = report("sha256-2048", 0.0, 1);
    let error = build_document("zkprof", "sha256", metadata(), &[(&report, None)])
        .expect_err("must refuse");
    assert_eq!(
        error,
        ExportError::NoDuration {
            workload_id: "sha256-2048".to_owned()
        }
    );
}

#[test]
fn a_document_publishes_one_system_and_every_measurement() {
    let small = report("sha256-128", 100.0, 1);
    let large = report("sha256-2048", 2_000.0, 1);
    let document = build_document(
        "zkprof-arkworks-groth16",
        "sha256",
        metadata(),
        &[(&small, Some(128)), (&large, Some(2048))],
    )
    .expect("a document");

    assert_eq!(document.measurements.len(), 2);
    assert_eq!(document.systems.len(), 1);

    let system = document
        .systems
        .get("zkprof-arkworks-groth16")
        .expect("present");
    // Only what the report justifies. security_bits is a declaration a tool
    // must not make on a submitter's behalf.
    assert_eq!(system.proving_system, "groth16");
    assert_eq!(system.field_curve, "bn254");
    assert_eq!(system.security_bits, None);
    assert_eq!(system.iop, None);

    let json = serde_json::to_string(&document).expect("serialisable");
    assert!(
        !json.contains("security_bits"),
        "must not assert a security level"
    );
}

#[test]
fn every_canonical_target_the_crate_advertises_is_accepted() {
    for target in CANONICAL_TARGETS {
        let report = report(&format!("{target}-128"), 100.0, 1);
        build_document("zkprof", target, metadata(), &[(&report, Some(128))])
            .unwrap_or_else(|error| panic!("{target} should be canonical: {error}"));
    }
}

/// Epoch seconds for the dates below, taken from `date -u -d <date> +%s`
/// rather than from any arithmetic in this file.
///
/// They are named constants and not inline literals because clippy's
/// `duration_suboptimal_units` rewrites an inline second-literal that happens
/// to be a whole number of hours, and the resulting `from_hours(483576)`
/// hides which date is being asserted.
const SECONDS_2024_02_29: u64 = 1_709_164_800;
const SECONDS_2025_03_02: u64 = 1_740_873_600;
const SECONDS_2026_08_30: u64 = 1_788_048_000;
const ONE_DAY: u64 = 86_400;

#[test]
fn rfc3339_formats_the_epoch_and_a_known_date() {
    assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    assert_eq!(
        rfc3339_utc(UNIX_EPOCH + Duration::from_secs(SECONDS_2026_08_30)),
        "2026-08-30T00:00:00Z"
    );
    // A leap day, and the day after it, are where hand-rolled date maths
    // usually breaks: the year length changes under the increment.
    assert_eq!(
        rfc3339_utc(UNIX_EPOCH + Duration::from_secs(SECONDS_2024_02_29)),
        "2024-02-29T00:00:00Z"
    );
    assert_eq!(
        rfc3339_utc(UNIX_EPOCH + Duration::from_secs(SECONDS_2024_02_29 + ONE_DAY)),
        "2024-03-01T00:00:00Z"
    );
    // 2025 is not a leap year, so the same calendar date is one day closer to
    // the epoch than 2024's was.
    assert_eq!(
        rfc3339_utc(UNIX_EPOCH + Duration::from_secs(SECONDS_2025_03_02)),
        "2025-03-02T00:00:00Z"
    );
}
