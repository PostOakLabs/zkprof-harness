// SPDX-License-Identifier: Apache-2.0

//! The `FibRace` result shape, and the nearest-faithful view of a zkprof report
//! as one.
//!
//! FibRace (arXiv:2510.14693, retrieved 2026-08-31) is the nearest published
//! sibling to zkprof: a large-scale *mobile* client-side proving benchmark
//! (Cairo M on smartphones, ~2.2M proofs, 1,420 device models). It reports, per
//! player session, a proof duration, a proving frequency (Cairo M cycles/sec),
//! device metadata (model, OS, RAM, SoC), a security level, and a reliability
//! figure. zkprof reports a *timing attribution* of one proving run on one
//! target.
//!
//! # What maps, and what is an explicit null
//!
//! The overlap between the two shapes is duration, memory, and target identity.
//! Everything FibRace reports that a zkprof report does not measure is emitted
//! **explicitly as `null`** with a [`MappingNote`] — never silently dropped,
//! never guessed. This is the opposite convention from [`crate::csp`], which
//! *omits* unmeasurable fields: FibRace's view is a first-class schema of its
//! own, so a reader must be able to see the field and read why it is empty,
//! rather than assume it was never considered.
//!
//! | FibRace field | from a zkprof report | note |
//! |---|---|---|
//! | proving time | `total_ms` | the measured region |
//! | memory | `memory.peak_heap_bytes` | measured on wasm |
//! | device | `target` | browser / os / cpu |
//! | proving system | `system.proof_system` | zkprof proves Groth16/BN254; FibRace used Cairo M/M31 |
//! | proving_frequency_hz | `null` | Cairo M cycles/sec; zkprof never measures it |
//! | device_model | `null` | zkprof records browser/os/cpu, not a device model |
//! | soc | `null` | never measured |
//! | minimum_ram_gb | `null` | a FibRace lower-bound threshold, not measured here |
//! | security_bits | `null` | a declaration about the system, not a measurement |
//! | crash_rate | `null` | never measured |
//!
//! **Zero browser performance numbers exist** for zkprof in the sense FibRace
//! has them; a `fibrace-view` of a zkprof report is a *shape*, not a
//! comparison. This module makes no performance claim about either system. The
//! two are complementary on purpose (mobile-native vs browser) and this view
//! only exists so a reader can put a zkprof run next to the FibRace schema
//! without inventing the fields the report does not carry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::Report;

/// The FibRace source this view is shaped against. Retrieved 2026-08-31 (read
/// from the arXiv abstract / published report structure; no text was obtained
/// that was not public).
pub const FIBRACE_PAPER: &str =
    "FibRace: a large-scale benchmark of client-side proving on mobile devices";
/// Authors of the FibRace paper (KKRT Labs / Hyli).
pub const FIBRACE_AUTHORS: &str = "Malatrait, Sirac (KKRT Labs / Hyli)";
/// arXiv landing page for the FibRace paper.
pub const FIBRACE_URL: &str = "https://arxiv.org/abs/2510.14693";
/// Date the FibRace paper/report was retrieved.
pub const FIBRACE_READ_DATE: &str = "2026-08-31";

/// Why one FibRace field is `null` in a fibrace-view of a zkprof report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MappingNote {
    /// The FibRace field a zkprof report cannot fill.
    pub field: String,
    /// Why it is null, stated plainly.
    pub why: String,
}

/// A FibRace-only field a zkprof report does not measure, and what it would
/// take to fill it (kept as data so a later session can see the whole gap).
pub const NULL_FIELDS: &[(&str, &str)] = &[
    (
        "proving_frequency_hz",
        "FibRace records Cairo M cycles/sec (mean ~30 kHz, median ~10 kHz). zkprof measures wall time, never a cycles/sec rate.",
    ),
    (
        "device_model",
        "FibRace records a phone/tablet model. zkprof records browser, OS and CPU only; it does not probe a device model string.",
    ),
    (
        "soc",
        "FibRace records the SoC. zkprof does not probe a SoC identifier.",
    ),
    (
        "minimum_ram_gb",
        "FibRace's 3 GB minimum-RAM threshold is a benchmark finding, not a measurement a zkprof report carries.",
    ),
    (
        "security_bits",
        "A declaration about the proving system (FibRace used ~96-bit). zkprof measures time, never asserts a security level; supplying it is a human claim.",
    ),
    (
        "crash_rate",
        "FibRace's 0.72% crash rate is a campaign reliability figure; a zkprof report records no failures.",
    ),
    (
        "benchmark_valid",
        "FibRace's validity gate (generated, verified, settled onchain) is a pipeline property, not a per-run measurement.",
    ),
];

/// Whether the `proving_system` / `field` match a FibRace-Cairo-M-style system.
///
/// A zkprof report proves Groth16/BN254; FibRace proved Cairo M on the M31
/// field. They are different systems, and this view says so rather than
/// pretending they are the same workload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemInfo {
    /// The proof system a zkprof report was produced by.
    pub proof_system: String,
    /// The curve/field of that system.
    pub field: String,
    /// Whether this matches FibRace's Cairo-M-on-M31 pipeline. It does not.
    pub matches_fibrace_cairo_m: bool,
}

/// Target identity, from a zkprof report's `target`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct DeviceInfo {
    /// Broad target class (`wasm-scalar`, `webgpu`, `native`, ...).
    pub kind: String,
    /// Browser name + version, when present.
    pub browser: Option<String>,
    /// Operating system string.
    pub os: String,
    /// CPU / architecture string.
    pub cpu: String,
}

/// The nearest-faithful FibRace view of one zkprof report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct FibraceView {
    /// What this document is.
    pub provenance: String,
    /// The FibRace paper the shape is read from.
    pub source_paper: String,
    /// Workload identity from the zkprof report.
    pub workload_id: String,
    /// Inner system the zkprof report measured.
    pub system: SystemInfo,
    /// Wall time of the measured region, milliseconds (= zkprof `total_ms`).
    pub proving_time_ms: f64,
    /// Peak heap in bytes, when measured.
    pub memory_peak_bytes: Option<u64>,
    /// Target/deployment identity.
    pub device: DeviceInfo,
    /// Iterations inside the measured region.
    pub iterations: u32,
    /// Primitive breakdown, name -> ms, in schema order.
    pub primitives_ms: BTreeMap<String, f64>,
    /// FibRace's Cairo M cycles/sec; `null` because zkprof measures wall time only.
    pub proving_frequency_hz: Option<f64>,
    /// FibRace's device model; `null` because zkprof records browser/os/cpu.
    pub device_model: Option<String>,
    /// FibRace's SoC; `null` because zkprof never probes one.
    pub soc: Option<String>,
    /// FibRace's minimum-RAM lower bound; `null` — a benchmark finding, not a measurement.
    pub minimum_ram_gb: Option<u64>,
    /// FibRace's security level; `null` — a declaration, not a measurement.
    pub security_bits: Option<u64>,
    /// FibRace's crash rate; `null` — a campaign reliability figure.
    pub crash_rate: Option<f64>,
    /// FibRace's validity gate; `null` — a pipeline property, not a per-run measurement.
    pub benchmark_valid: Option<bool>,
    /// Why each null is null.
    pub mapping_notes: Vec<MappingNote>,
}

impl FibraceView {
    /// Build the view from a report. Never panics; fields a report lacks become
    /// `None` and are explained in [`Self::mapping_notes`].
    #[must_use]
    pub fn from_report(report: &Report) -> Self {
        let mut primitives_ms = BTreeMap::new();
        for row in &report.primitives {
            primitives_ms.insert(row.name.to_string(), row.ms);
        }
        let mapping_notes = NULL_FIELDS
            .iter()
            .map(|(field, why)| MappingNote {
                field: (*field).to_owned(),
                why: (*why).to_owned(),
            })
            .collect();

        Self {
            provenance: "fibrace-view of a zkprof report".to_owned(),
            source_paper: format!(
                "{FIBRACE_PAPER} — {FIBRACE_AUTHORS} ({FIBRACE_URL}, read {FIBRACE_READ_DATE})"
            ),
            workload_id: report.workload_id.clone(),
            system: SystemInfo {
                proof_system: report.system.proof_system.clone(),
                field: report.system.curve_or_field.clone(),
                matches_fibrace_cairo_m: report.system.proof_system.eq_ignore_ascii_case("cairo-m")
                    && report.system.curve_or_field.eq_ignore_ascii_case("m31"),
            },
            proving_time_ms: report.total_ms,
            memory_peak_bytes: report.memory.peak_heap_bytes,
            device: DeviceInfo {
                kind: report.target.kind.to_string(),
                browser: report.target.browser.clone(),
                os: report.target.os.clone(),
                cpu: report.target.cpu.clone(),
            },
            iterations: report.iterations,
            primitives_ms,
            proving_frequency_hz: None,
            device_model: None,
            soc: None,
            minimum_ram_gb: None,
            security_bits: None,
            crash_rate: None,
            benchmark_valid: None,
            mapping_notes,
        }
    }
}

/// The list of FibRace fields a zkprof report cannot fill, for the CLI to print
/// at export time so the gap is visible before anyone reads the view as a
/// comparison.
#[must_use]
pub fn null_field_notes() -> Vec<MappingNote> {
    NULL_FIELDS
        .iter()
        .map(|(field, why)| MappingNote {
            field: (*field).to_owned(),
            why: (*why).to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Memory, Phase, Report, System, Target, TargetKind, Threads};

    fn fixture_report() -> Report {
        Report {
            schema: None,
            report_version: "0.1.0".to_owned(),
            target: Target {
                kind: TargetKind::WasmSimd,
                browser: Some("headless chrome".to_owned()),
                os: "Win32".to_owned(),
                cpu: "wasm32".to_owned(),
                gpu: None,
            },
            system: System {
                proof_system: "groth16".to_owned(),
                curve_or_field: "bn254".to_owned(),
                constraint_count: Some(15_553),
            },
            workload_id: "groth16-bn254-merkle-poseidon-d64".to_owned(),
            total_ms: 3_101.1,
            phases: Vec::<Phase>::new(),
            primitives: vec![
                crate::model::PrimitiveRow {
                    name: crate::model::Primitive::Msm,
                    ms: 2_589.1,
                    share: 0.836,
                },
                crate::model::PrimitiveRow {
                    name: crate::model::Primitive::WitnessGen,
                    ms: 338.4,
                    share: 0.109,
                },
            ],
            memory: Memory {
                peak_heap_bytes: Some(69_795_840),
            },
            threads: Threads {
                count: 1,
                shared_array_buffer: false,
            },
            cross_origin_isolated: true,
            timer_resolution_us: 5.0,
            iterations: 1,
            run_manifest_ref: None,
            gpu_timing_source: None,
            backend_id: None,
            version: None,
            commitment_scheme: None,
        }
    }

    #[test]
    fn view_maps_duration_memory_and_device() {
        let view = FibraceView::from_report(&fixture_report());
        assert!((view.proving_time_ms - 3_101.1).abs() < 1e-6);
        assert_eq!(view.memory_peak_bytes, Some(69_795_840));
        assert_eq!(view.device.kind, "wasm-simd");
        assert_eq!(view.device.os, "Win32");
        assert!((view.primitives_ms.get("msm").copied().unwrap_or(0.0) - 2_589.1).abs() < 1e-6);
        assert!(!view.system.matches_fibrace_cairo_m);
    }

    #[test]
    fn fibrace_only_fields_are_explicit_nulls() {
        let view = FibraceView::from_report(&fixture_report());
        assert!(view.proving_frequency_hz.is_none());
        assert!(view.device_model.is_none());
        assert!(view.security_bits.is_none());
        assert!(view.crash_rate.is_none());
        // Every null field has a mapping note; none are silently absent.
        assert_eq!(view.mapping_notes.len(), NULL_FIELDS.len());
        let note_fields: Vec<&str> = view
            .mapping_notes
            .iter()
            .map(|n| n.field.as_str())
            .collect();
        assert!(note_fields.contains(&"proving_frequency_hz"));
        assert!(note_fields.contains(&"security_bits"));
        assert!(note_fields.contains(&"crash_rate"));
    }

    #[test]
    fn view_round_trips_through_json() {
        let view = FibraceView::from_report(&fixture_report());
        let json = serde_json::to_string(&view).expect("serialize");
        let back: FibraceView = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(view, back);
    }
}
