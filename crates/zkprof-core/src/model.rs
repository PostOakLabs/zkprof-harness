// SPDX-License-Identifier: Apache-2.0

//! The Rust transcription of `zkprof-report.schema.json`.
//!
//! Every struct here is `Serialize`/`Deserialize` against the exact JSON shape
//! the schema declares: same keys, same enum spellings, same `additionalProperties:
//! false` discipline (no struct carries a field the schema does not name).
//!
//! `tests/schema_alignment.rs` reads the vendored schema and checks the `required`
//! list and both enum blocks against what these structs emit, so a schema change
//! in `edi-schema` fails the build here rather than silently producing reports
//! the validator rejects.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Tolerance the Python validator allows on the sum of primitive shares.
///
/// Duplicated from `edi-schema`'s validator deliberately: [`Report::validate`]
/// performs the same check in-process so a bad report is caught at the point of
/// construction rather than at the point of publication.
pub const SHARE_TOLERANCE: f64 = 0.02;

/// One profiling run of one proving workload on one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Report {
    /// Self-identifying reference to the schema. Emitted so `edi-validate` can
    /// pick the document kind without being told.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Semantic version of this report schema.
    pub report_version: String,

    /// What the workload ran on.
    pub target: Target,

    /// What the workload was.
    pub system: System,

    /// Identifies the workload. Fixture reports say so in this field.
    pub workload_id: String,

    /// Wall time of the measured region, milliseconds.
    pub total_ms: f64,

    /// Sequential prover phases. Shares are relative to `total_ms` and are not
    /// required to partition it.
    pub phases: Vec<Phase>,

    /// Primitive-level attribution. Shares are relative to the sum of
    /// `primitives` and must sum to 1.0 within [`SHARE_TOLERANCE`].
    pub primitives: Vec<PrimitiveRow>,

    /// Heap high-water mark, when measured.
    pub memory: Memory,

    /// Threading configuration the measurement ran under.
    pub threads: Threads,

    /// Value of the platform's `crossOriginIsolated` flag.
    #[serde(rename = "crossOriginIsolated")]
    pub cross_origin_isolated: bool,

    /// Measured minimum observable nonzero delta of the clock source, µs.
    pub timer_resolution_us: f64,

    /// Number of workload repetitions inside the measured region. Per-operation
    /// figures are derived by dividing `ms` by this.
    pub iterations: u32,

    /// Optional reference to an `edi-schema` run-manifest document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_manifest_ref: Option<String>,

    /// How the GPU-attributed time in this report was obtained.
    ///
    /// Absent means the report attributes no time to a GPU at all. When it is
    /// present it says which instrument produced the figure — see
    /// [`GpuTimingSource`] for why that is not something a reader can infer
    /// from the number itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_timing_source: Option<GpuTimingSource>,

    /// Identity of the proving backend that produced this report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_id: Option<String>,

    /// Backend implementation version, if the backend exposes one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Polynomial commitment scheme the backend uses, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commitment_scheme: Option<String>,
}

/// The instrument behind a report's GPU-attributed time.
///
/// # Why this travels with the report
///
/// The two sources measure different things, and the difference is not a
/// rounding error:
///
/// * **`timestamp-query`** — WebGPU's optional `timestamp-query` feature. A
///   `GPUQuerySet` records a timestamp at the start of the compute pass and
///   another at its end; the figure is the resolved difference. It measures
///   *the pass*.
/// * **`wall-clock`** — the host clock, spanning the moment the command buffer
///   was submitted to the moment the device reported the submission finished.
///   It measures *the whole submission round trip*.
///
/// A `timestamp-query` figure is strictly narrower than a wall-clock one: it
/// cannot see buffer copies made outside the pass, resource creation, or
/// submission latency. That is precisely why a run that has the feature
/// reports both — the gap between the two numbers is the overhead, and neither
/// number on its own separates that overhead from the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuTimingSource {
    /// A `GPUQuerySet` spanned the pass and the figure is the resolved
    /// timestamp delta. Pass-level only.
    #[serde(rename = "timestamp-query")]
    TimestampQuery,
    /// The host clock spanned submission and completion, so queue and submit
    /// overhead are inside the figure whether or not they belong there.
    #[serde(rename = "wall-clock")]
    WallClock,
}

impl GpuTimingSource {
    /// Every variant, in declaration order.
    pub const ALL: [Self; 2] = [Self::TimestampQuery, Self::WallClock];

    /// The spelling this variant has in JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TimestampQuery => "timestamp-query",
            Self::WallClock => "wall-clock",
        }
    }
}

impl fmt::Display for GpuTimingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for GpuTimingSource {
    type Err = UnknownVariant;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|source| source.as_str() == s)
            .ok_or_else(|| UnknownVariant(s.to_owned()))
    }
}

/// Where the workload ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// Broad target class.
    pub kind: TargetKind,

    /// Browser name and version, when the target is a browser target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,

    /// Operating system.
    pub os: String,

    /// CPU. On native targets this is the architecture (`x86_64`, `aarch64`);
    /// this crate does not probe CPU model strings.
    pub cpu: String,

    /// Adapter string reported by the platform, for `webgpu` targets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
}

impl Target {
    /// A native target filled in from compile-time constants.
    ///
    /// `os` and `cpu` come from [`std::env::consts`]. Neither is a runtime
    /// probe, so a binary cross-compiled elsewhere will report its build
    /// target; that is the honest description of what was compiled, if not of
    /// what ran it.
    #[must_use]
    pub fn native() -> Self {
        Self {
            kind: TargetKind::Native,
            browser: None,
            os: std::env::consts::OS.to_owned(),
            cpu: std::env::consts::ARCH.to_owned(),
            gpu: None,
        }
    }

    /// A native target with an explicit CPU description.
    #[must_use]
    pub fn native_with_cpu(cpu: impl Into<String>) -> Self {
        Self {
            cpu: cpu.into(),
            ..Self::native()
        }
    }
}

/// Broad target class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetKind {
    /// A native binary.
    #[serde(rename = "native")]
    Native,
    /// WebAssembly, scalar build.
    #[serde(rename = "wasm-scalar")]
    WasmScalar,
    /// WebAssembly built with `+simd128`.
    #[serde(rename = "wasm-simd")]
    WasmSimd,
    /// WebAssembly with threads and a `SharedArrayBuffer`.
    #[serde(rename = "wasm-threads")]
    WasmThreads,
    /// Compute work offloaded to WebGPU.
    #[serde(rename = "webgpu")]
    Webgpu,
}

impl TargetKind {
    /// Every variant, in declaration order.
    pub const ALL: [Self; 5] = [
        Self::Native,
        Self::WasmScalar,
        Self::WasmSimd,
        Self::WasmThreads,
        Self::Webgpu,
    ];

    /// The spelling this variant has in JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::WasmScalar => "wasm-scalar",
            Self::WasmSimd => "wasm-simd",
            Self::WasmThreads => "wasm-threads",
            Self::Webgpu => "webgpu",
        }
    }
}

impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TargetKind {
    type Err = UnknownVariant;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| UnknownVariant(s.to_owned()))
    }
}

/// What the workload was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct System {
    /// e.g. `groth16`, `plonk`.
    pub proof_system: String,

    /// e.g. `bn254`, `bls12-381`, `m31`, `goldilocks`.
    pub curve_or_field: String,

    /// Number of constraints in the circuit, when known. Absent rather than 0
    /// when not measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraint_count: Option<u64>,
}

impl System {
    /// A system description with no constraint count.
    #[must_use]
    pub fn new(proof_system: impl Into<String>, curve_or_field: impl Into<String>) -> Self {
        Self {
            proof_system: proof_system.into(),
            curve_or_field: curve_or_field.into(),
            constraint_count: None,
        }
    }

    /// Attaches a constraint count.
    #[must_use]
    pub fn with_constraint_count(mut self, constraint_count: u64) -> Self {
        self.constraint_count = Some(constraint_count);
        self
    }
}

/// A sequential prover phase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    /// Free-form phase name.
    pub name: String,

    /// Wall time of the phase, milliseconds.
    pub ms: f64,

    /// Fraction of `total_ms`. Not required to partition it: phases may
    /// overlap or nest.
    pub share: f64,
}

/// One row of primitive-level attribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrimitiveRow {
    /// Which primitive this row attributes time to.
    pub name: Primitive,

    /// Accumulated time in this primitive, milliseconds.
    pub ms: f64,

    /// Fraction of the total primitive time. These shares do sum to 1.
    pub share: f64,
}

/// The seven primitives the report schema attributes time to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    /// Multi-scalar multiplication.
    Msm,
    /// Number-theoretic transforms and FFTs.
    NttFft,
    /// Hash functions: Poseidon, Rescue, Keccak, …
    Hash,
    /// Field and curve arithmetic not covered by MSM or NTT.
    FieldArith,
    /// Moving data across the host/WASM boundary, and serialisation.
    Marshalling,
    /// Witness generation.
    WitnessGen,
    /// Anything not attributable to one of the above.
    ///
    /// A large `other` share is a statement about instrumentation coverage,
    /// not about performance. W3.2 records how much of the prover falls here.
    Other,
}

impl Primitive {
    /// Every variant, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::Msm,
        Self::NttFft,
        Self::Hash,
        Self::FieldArith,
        Self::Marshalling,
        Self::WitnessGen,
        Self::Other,
    ];

    /// The spelling this variant has in JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Msm => "msm",
            Self::NttFft => "ntt_fft",
            Self::Hash => "hash",
            Self::FieldArith => "field_arith",
            Self::Marshalling => "marshalling",
            Self::WitnessGen => "witness_gen",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for Primitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Primitive {
    type Err = UnknownVariant;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|primitive| primitive.as_str() == s)
            .ok_or_else(|| UnknownVariant(s.to_owned()))
    }
}

/// Heap high-water mark.
///
/// Serialises to `{}` when nothing was measured. The schema's note is explicit:
/// omit the key rather than reporting 0, because 0 reads as a measurement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    /// Peak heap size in bytes, when measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_heap_bytes: Option<u64>,
}

impl Memory {
    /// No heap measurement was taken.
    #[must_use]
    pub fn unmeasured() -> Self {
        Self {
            peak_heap_bytes: None,
        }
    }

    /// A measured peak heap, in bytes.
    #[must_use]
    pub fn peak_heap_bytes(bytes: u64) -> Self {
        Self {
            peak_heap_bytes: Some(bytes),
        }
    }
}

/// Threading configuration the measurement ran under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Threads {
    /// Worker count. 1 for a single-threaded build.
    pub count: u32,

    /// Whether a `SharedArrayBuffer` backed the workers.
    pub shared_array_buffer: bool,
}

impl Threads {
    /// A single-threaded run.
    #[must_use]
    pub fn single() -> Self {
        Self {
            count: 1,
            shared_array_buffer: false,
        }
    }

    /// `count` workers sharing a `SharedArrayBuffer`.
    #[must_use]
    pub fn shared(count: u32) -> Self {
        Self {
            count,
            shared_array_buffer: true,
        }
    }

    /// `count` workers that do not share memory.
    #[must_use]
    pub fn parallel(count: u32) -> Self {
        Self {
            count,
            shared_array_buffer: false,
        }
    }
}

/// A string that is not one of the schema's enum values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownVariant(pub String);

impl fmt::Display for UnknownVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown variant: {:?}", self.0)
    }
}

impl std::error::Error for UnknownVariant {}

/// Why a [`Report`] cannot be published.
///
/// These are the structural invariants the schema cannot express in JSON
/// Schema alone — most importantly that primitive shares actually partition
/// the measured primitive time.
#[derive(Debug, Clone, PartialEq)]
pub enum ReportError {
    /// No primitive span recorded any elapsed time, so shares are undefined.
    NoMeasuredTime,
    /// `primitives` was empty. The schema requires at least one row.
    NoPrimitives,
    /// A span was still open when the report was assembled.
    ///
    /// Its duration has not been recorded, so the report would under-attribute
    /// the run by an unknown amount.
    SpanStillOpen {
        /// The label of the span that never closed.
        name: String,
    },
    /// A phase or primitive recorded a negative or non-finite duration.
    BadDuration {
        /// Which row.
        name: String,
        /// The offending value, milliseconds.
        ms: f64,
    },
    /// A share fell outside `[0, 1]`.
    ShareOutOfRange {
        /// Which row.
        name: String,
        /// The offending value.
        share: f64,
    },
    /// Primitive shares did not sum to 1.0 within [`SHARE_TOLERANCE`].
    PrimitiveShareSum {
        /// What they summed to.
        sum: f64,
    },
    /// `workload_id` was empty.
    EmptyWorkloadId,
    /// `iterations` was zero.
    ZeroIterations,
    /// `timer_resolution_us` was negative or non-finite.
    BadTimerResolution {
        /// The offending value, µs.
        timer_resolution_us: f64,
    },
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMeasuredTime => {
                f.write_str("no primitive recorded any elapsed time; shares would be undefined")
            }
            Self::NoPrimitives => f.write_str("`primitives` is empty; the schema requires one row"),
            Self::SpanStillOpen { name } => write!(
                f,
                "span {name:?} was still open; its duration would be missing from the report"
            ),
            Self::BadDuration { name, ms } => {
                write!(f, "row {name:?} recorded a duration of {ms} ms")
            }
            Self::ShareOutOfRange { name, share } => {
                write!(
                    f,
                    "row {name:?} recorded a share of {share}, outside [0, 1]"
                )
            }
            Self::PrimitiveShareSum { sum } => write!(
                f,
                "primitive shares sum to {sum}, which is more than {SHARE_TOLERANCE} from 1.0"
            ),
            Self::EmptyWorkloadId => f.write_str("`workload_id` is empty"),
            Self::ZeroIterations => f.write_str("`iterations` is zero"),
            Self::BadTimerResolution {
                timer_resolution_us,
            } => write!(
                f,
                "`timer_resolution_us` is {timer_resolution_us}, which is negative or not finite"
            ),
        }
    }
}

impl std::error::Error for ReportError {}

impl Report {
    /// Serialises to compact JSON.
    ///
    /// # Errors
    ///
    /// Propagates [`serde_json::Error`]. With the types in this module it
    /// cannot fail in practice; the signature exists so callers do not have to
    /// assume that.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialises to indented JSON.
    ///
    /// # Errors
    ///
    /// Propagates [`serde_json::Error`].
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialises from JSON.
    ///
    /// This does **not** run [`Report::validate`]; a document can be
    /// well-formed JSON, match the struct, and still violate the share rules.
    /// Call `validate` before trusting one.
    ///
    /// # Errors
    ///
    /// Propagates [`serde_json::Error`].
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Checks the structural invariants JSON Schema cannot express here.
    ///
    /// # Errors
    ///
    /// Returns the first [`ReportError`] found.
    pub fn validate(&self) -> Result<(), ReportError> {
        if self.workload_id.is_empty() {
            return Err(ReportError::EmptyWorkloadId);
        }
        if self.iterations == 0 {
            return Err(ReportError::ZeroIterations);
        }
        // Spelled out rather than negated so NaN lands here too: a NaN
        // resolution compares `false` against everything, including itself.
        if !self.timer_resolution_us.is_finite() || self.timer_resolution_us < 0.0 {
            return Err(ReportError::BadTimerResolution {
                timer_resolution_us: self.timer_resolution_us,
            });
        }
        if self.primitives.is_empty() {
            return Err(ReportError::NoPrimitives);
        }
        for row in &self.primitives {
            check_duration(row.name.as_str(), row.ms)?;
            check_share(row.name.as_str(), row.share)?;
        }
        for phase in &self.phases {
            check_duration(&phase.name, phase.ms)?;
            check_share(&phase.name, phase.share)?;
        }

        let sum: f64 = self.primitives.iter().map(|row| row.share).sum();
        if (sum - 1.0).abs() > SHARE_TOLERANCE {
            return Err(ReportError::PrimitiveShareSum { sum });
        }

        Ok(())
    }

    /// The row for `primitive`, if the report has one.
    #[must_use]
    pub fn primitive(&self, primitive: Primitive) -> Option<&PrimitiveRow> {
        self.primitives.iter().find(|row| row.name == primitive)
    }

    /// The phase named `name`, if the report has one.
    #[must_use]
    pub fn phase(&self, name: &str) -> Option<&Phase> {
        self.phases.iter().find(|phase| phase.name == name)
    }

    /// Sum of all primitive rows, milliseconds.
    ///
    /// This is the denominator primitive shares are taken over, which is not
    /// necessarily `total_ms`: time outside any span belongs to neither.
    #[must_use]
    pub fn attributed_ms(&self) -> f64 {
        self.primitives.iter().map(|row| row.ms).sum()
    }

    /// Primitive time not attributed to a named primitive, milliseconds.
    ///
    /// `total_ms - attributed_ms`. Positive when the measured region includes
    /// work no span covered; negative when primitive spans overlap.
    #[must_use]
    pub fn unattributed_ms(&self) -> f64 {
        self.total_ms - self.attributed_ms()
    }
}

fn check_duration(name: &str, ms: f64) -> Result<(), ReportError> {
    if ms.is_finite() && ms >= 0.0 {
        Ok(())
    } else {
        Err(ReportError::BadDuration {
            name: name.to_owned(),
            ms,
        })
    }
}

fn check_share(name: &str, share: f64) -> Result<(), ReportError> {
    if share.is_finite() && (0.0..=1.0).contains(&share) {
        Ok(())
    } else {
        Err(ReportError::ShareOutOfRange {
            name: name.to_owned(),
            share,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Memory, Primitive, Report, ReportError, System, Target, TargetKind, Threads,
        SHARE_TOLERANCE,
    };

    fn sample_report() -> Report {
        Report {
            schema: None,
            report_version: "0.1.0".to_owned(),
            target: Target::native(),
            system: System::new("groth16", "bn254"),
            workload_id: "test".to_owned(),
            total_ms: 10.0,
            phases: vec![],
            primitives: vec![
                super::PrimitiveRow {
                    name: Primitive::Msm,
                    ms: 7.5,
                    share: 0.75,
                },
                super::PrimitiveRow {
                    name: Primitive::Other,
                    ms: 2.5,
                    share: 0.25,
                },
            ],
            memory: Memory::default(),
            threads: Threads::single(),
            cross_origin_isolated: false,
            timer_resolution_us: 0.1,
            iterations: 1,
            run_manifest_ref: None,
            gpu_timing_source: None,
            backend_id: None,
            version: None,
            commitment_scheme: None,
        }
    }

    #[test]
    fn a_well_formed_report_validates() {
        sample_report().validate().expect("report should validate");
    }

    #[test]
    fn primitive_shares_must_partition() {
        let mut report = sample_report();
        report.primitives[0].share = 0.5;
        report.primitives[1].share = 0.2;

        assert_eq!(
            report.validate(),
            Err(ReportError::PrimitiveShareSum { sum: 0.7 })
        );
    }

    #[test]
    fn tolerance_is_the_same_one_the_python_validator_uses() {
        let mut report = sample_report();
        report.primitives[0].share = 0.75 + SHARE_TOLERANCE / 2.0;
        report.validate().expect("within tolerance");
    }

    #[test]
    fn nan_is_rejected_rather_than_passing_a_range_check() {
        let mut report = sample_report();
        report.timer_resolution_us = f64::NAN;
        assert!(matches!(
            report.validate(),
            Err(ReportError::BadTimerResolution { .. })
        ));

        let mut report = sample_report();
        report.primitives[0].ms = f64::NAN;
        assert!(matches!(
            report.validate(),
            Err(ReportError::BadDuration { .. })
        ));
    }

    #[test]
    fn an_empty_primitives_list_is_rejected() {
        let mut report = sample_report();
        report.primitives.clear();
        assert_eq!(report.validate(), Err(ReportError::NoPrimitives));
    }

    #[test]
    fn unmeasured_memory_serialises_to_an_empty_object() {
        let memory = Memory::unmeasured();
        assert_eq!(serde_json::to_string(&memory).unwrap(), "{}");
    }

    #[test]
    fn an_unmeasured_constraint_count_is_omitted_not_zero() {
        let system = System::new("plonk", "bls12-381");
        let json = serde_json::to_value(&system).unwrap();
        assert!(json.get("constraint_count").is_none());

        let with = system.with_constraint_count(16_384);
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["constraint_count"], 16_384);
    }

    #[test]
    fn target_kinds_round_trip_through_their_json_spelling() {
        for kind in TargetKind::ALL {
            assert_eq!(kind.as_str().parse::<TargetKind>().unwrap(), kind);
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, serde_json::json!(kind.as_str()));
        }
    }

    #[test]
    fn primitives_round_trip_through_their_json_spelling() {
        for primitive in Primitive::ALL {
            assert_eq!(primitive.as_str().parse::<Primitive>().unwrap(), primitive);
            let json = serde_json::to_value(primitive).unwrap();
            assert_eq!(json, serde_json::json!(primitive.as_str()));
        }
    }

    #[test]
    fn an_unknown_variant_is_an_error_not_a_panic() {
        assert!("nope".parse::<Primitive>().is_err());
        assert!("wasm".parse::<TargetKind>().is_err());
    }

    #[test]
    fn unattributed_time_is_total_minus_attributed() {
        let report = sample_report();
        // 10 ms total, 7.5 + 2.5 attributed.
        assert!((report.unattributed_ms() - 0.0).abs() < 1e-9);

        let mut report = report;
        report.total_ms = 12.0;
        assert!((report.unattributed_ms() - 2.0).abs() < 1e-9);
    }
}
