// SPDX-License-Identifier: Apache-2.0

//! The `zkprof` binary.
//!
//! Three subcommands:
//! - `prove` runs a backend, prints a breakdown and writes a `zkprof-report` document.
//! - `render` reads a `zkprof-report` document and prints its breakdown as a table.
//! - `diff` reads two `zkprof-report` documents and prints the per-primitive delta.
//!
//! `render` and `diff` are the read side of the same document `prove` writes;
//! they are how a report is inspected without re-running the workload.
//!
//! # What is printed versus what is written
//!
//! The printed table is per-iteration; the JSON holds the totals for the
//! whole run. With one iteration they are the same number, which is the
//! common case for a proof this size. Printing per-iteration figures is what
//! makes two runs comparable, and the JSON keeping totals is what makes it a
//! faithful record of what actually happened.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use clap::{Parser, Subcommand, ValueEnum};
use zkprof_backend_arkworks::bench::{benchmark, default_workload_id, Outcome, DEFAULT_SEED};
use zkprof_backend_arkworks::circuit::DEFAULT_DEPTH;
use zkprof_backend_arkworks::{Circuit, DEFAULT_SHA256_INPUT_BYTES, SHA256_INPUT_SIZES};
use zkprof_core::csp::{
    build_document, detect_target, rfc3339_utc, CspMetadata, CANONICAL_TARGETS, UNMAPPABLE_FIELDS,
};
use zkprof_core::fibrace::{null_field_notes, FibraceView};
use zkprof_core::model::{Primitive, Report, Target};
use zkprof_core::scale::TARGET_RATIO;

#[derive(Parser)]
#[command(
    name = "zkprof",
    version,
    about = "Profile ZK proving backends and emit zkprof-report documents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a workload, break the time down by primitive, write a report.
    Prove(Prove),

    /// Read a zkprof-report document and print its breakdown as a table.
    ///
    /// Rejects a document that does not validate against the vendored
    /// zkprof-report schema: a table of a structurally invalid report would be
    /// misleading, so the tool refuses rather than guessing.
    Render(Render),

    /// Read two zkprof-report documents and print the per-primitive delta.
    ///
    /// Both inputs must validate against the vendored zkprof-report schema; a
    /// delta against a malformed report is meaningless, so the tool refuses.
    Diff(Diff),

    /// Convert zkprof-report documents into another tool's result format.
    ///
    /// Read-only conversion: nothing here runs a workload, and nothing here
    /// submits anything anywhere.
    Export(Export),
}

/// The formats `zkprof export` can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ExportFormat {
    /// `ethereum/csp-benchmarks` — the PSE client-side-proving harness whose
    /// results are published on ethproofs.org.
    #[value(name = "csp-benchmarks")]
    CspBenchmarks,

    /// `FibRace` (arXiv:2510.14693) — the nearest-faithful view of a zkprof
    /// report in the mobile client-side-proving benchmark's result shape.
    /// FibRace-only fields are emitted as explicit nulls with a mapping note;
    /// no field is silently dropped or guessed.
    #[value(name = "fibrace-view")]
    FibraceView,
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            self.to_possible_value()
                .expect("variant has a value")
                .get_name(),
        )
    }
}

/// Arguments for `zkprof export`.
#[derive(clap::Args)]
struct Export {
    /// Output format.
    #[arg(long, value_enum)]
    format: ExportFormat,

    /// Report documents to convert.
    #[arg(long = "report", value_name = "REPORT", required = true, num_args = 1..)]
    reports: Vec<PathBuf>,

    /// Name to publish the system under in the output document.
    #[arg(long, value_name = "NAME", default_value = "zkprof-arkworks-groth16")]
    system: String,

    /// Canonical circuit the reports measured.
    ///
    /// Omit it and the tool tries to recognise one from each report's
    /// `workload_id`. It refuses rather than guessing: publishing a duration
    /// against a circuit nobody ran is worse than publishing nothing.
    #[arg(long, value_name = "TARGET")]
    target: Option<String>,

    /// Input size in the target's own unit — bytes for `sha256`, field
    /// elements for `poseidon`. Omit for fixed-size targets like `ecdsa`.
    #[arg(long, value_name = "N")]
    input_size: Option<u64>,

    /// Commit of the build that produced the measurements.
    #[arg(long, value_name = "SHA", default_value = "unknown")]
    commit_sha: String,

    /// Where to write the document. Prints to stdout when omitted.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
}

/// Which proving implementation to run.
///
/// One variant today. The enum exists so that adding a backend is adding a
/// variant, not editing every call site, and so that `--backend` in a script
/// keeps meaning the same thing when there is a second one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Backend {
    /// Groth16 over BN254, via `arkworks`.
    ArkworksGroth16,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            self.to_possible_value()
                .expect("variant has a value")
                .get_name(),
        )
    }
}

/// Which circuit to prove.
///
/// The names are the circuit names, not the workload names: `--circuit sha256`
/// is the csp-benchmarks `sha256` target and the `workload_id` written into
/// the report is derived from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CircuitArg {
    /// Depth-parameterised Merkle-Poseidon membership.
    #[value(name = "merkle-poseidon")]
    MerklePoseidon,
    /// Knowledge of a preimage of a public SHA-256 digest.
    Sha256,
}

impl std::fmt::Display for CircuitArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            self.to_possible_value()
                .expect("variant has a value")
                .get_name(),
        )
    }
}

/// Where to run it, using the same spellings as `target.kind` in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TargetArg {
    /// The host, natively compiled.
    Native,
    /// wasm32, scalar.
    #[value(name = "wasm-scalar")]
    WasmScalar,
    /// wasm32 with `simd128`.
    #[value(name = "wasm-simd")]
    WasmSimd,
    /// wasm32 with threads and `SharedArrayBuffer`.
    #[value(name = "wasm-threads")]
    WasmThreads,
    /// WebGPU compute.
    Webgpu,
}

impl std::fmt::Display for TargetArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            self.to_possible_value()
                .expect("variant has a value")
                .get_name(),
        )
    }
}

impl TargetArg {
    /// `None` when this target is wired up; otherwise why it is not.
    ///
    /// Every target is listed from the start so that asking for one gives an
    /// answer rather than an unknown-argument error. Refusing is the honest
    /// response: emitting a `wasm-simd` report from a native run would be
    /// fabricating a measurement.
    fn unimplemented_reason(self) -> Option<&'static str> {
        match self {
            Self::Native => None,
            // Not "not built yet": the wasm targets exist, but they are
            // reachable only through the browser harness, because a native
            // binary cannot run a wasm32 module. Pointing at the harness is
            // the useful answer; "unimplemented" would be a lie and would
            // leave the user with nowhere to go.
            Self::WasmScalar | Self::WasmSimd => Some(
                "this binary is natively compiled and cannot run a wasm32 \
                 module; wasm reports come from the browser harness — build \
                 with `python tools/build_wasm.py`, then serve with \
                 `python web/server.py`",
            ),
            Self::WasmThreads => Some(
                "wasm threads are a stretch goal: the module builds with \
                 `--features web-threads` and reports target.kind \
                 `wasm-threads`, but run_benchmark refuses every call — the \
                 worker pool is not implemented",
            ),
            Self::Webgpu => {
                Some("the WebGPU backend is W3.5 and is plumbing only, no measurements")
            }
        }
    }
}

#[derive(clap::Args)]
struct Prove {
    /// Proving implementation to run.
    #[arg(long, value_enum, default_value_t = Backend::ArkworksGroth16)]
    backend: Backend,

    /// Where to run it.
    #[arg(long, value_enum, default_value_t = TargetArg::Native)]
    target: TargetArg,

    /// Where to write the report.
    #[arg(long, value_name = "PATH")]
    out: PathBuf,

    /// Circuit to prove. `sha256` is a csp-benchmarks canonical circuit, so a
    /// number measured on it can be compared with that suite — and, run on
    /// both targets, it is the cross-target delta in one row.
    #[arg(long, value_enum, default_value_t = CircuitArg::MerklePoseidon)]
    circuit: CircuitArg,

    /// Merkle depth, and therefore the circuit size, for `--circuit
    /// merkle-poseidon`.
    ///
    /// The circuit has `243 * depth + 1` constraints.
    #[arg(long, default_value_t = DEFAULT_DEPTH)]
    depth: usize,

    /// Preimage length in bytes for `--circuit sha256`.
    ///
    /// Must be one of the sizes csp-benchmarks measures
    /// (128, 256, 512, 1024, 2048); a size their exporter would reject is not
    /// a size worth measuring.
    #[arg(long, default_value_t = DEFAULT_SHA256_INPUT_BYTES)]
    bytes: usize,

    /// Seed for the witness, the setup and the prover randomness.
    #[arg(long, default_value_t = DEFAULT_SEED)]
    seed: u64,

    /// `workload_id` to record. Defaults to one derived from the depth.
    #[arg(long, value_name = "ID")]
    workload_id: Option<String>,
}

/// Arguments for `zkprof render`.
#[derive(clap::Args)]
struct Render {
    /// Path to a `zkprof-report` document written by `zkprof prove`.
    #[arg(value_name = "REPORT")]
    path: PathBuf,
}

/// Arguments for `zkprof diff`.
#[derive(clap::Args)]
struct Diff {
    /// Baseline report (the "before" side of the delta).
    #[arg(value_name = "BASE")]
    base: PathBuf,

    /// Comparand report (the "after" side of the delta).
    #[arg(value_name = "COMPARE")]
    compare: PathBuf,
}

/// Reads a report document, parses it, and validates it.
///
/// The parse step (`Report::from_json`) only checks the JSON shape; the validate
/// step (`Report::validate`) checks the structural invariants JSON Schema cannot
/// express — shares partitioning the primitive time, a non-empty `workload_id`,
/// a finite non-negative clock resolution, and so on. Both must pass before the
/// CLI will render or diff a report: a table built from a document that fails
/// either check would present noise as a measurement.
fn load_report(path: &PathBuf) -> Result<Report, ExitCode> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("zkprof: could not read {}: {error}", path.display());
            return Err(ExitCode::FAILURE);
        }
    };

    let report = match Report::from_json(&text) {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "zkprof: {} is not a zkprof-report document: {error}",
                path.display()
            );
            return Err(ExitCode::FAILURE);
        }
    };

    if let Err(error) = report.validate() {
        eprintln!(
            "zkprof: {} is a malformed zkprof-report document: {error}",
            path.display()
        );
        return Err(ExitCode::FAILURE);
    }

    Ok(report)
}

fn render(args: &Render) -> ExitCode {
    let report = match load_report(&args.path) {
        Ok(report) => report,
        Err(code) => return code,
    };

    print_report(&report);
    ExitCode::SUCCESS
}

fn diff(args: &Diff) -> ExitCode {
    let base = match load_report(&args.base) {
        Ok(report) => report,
        Err(code) => return code,
    };
    let compare = match load_report(&args.compare) {
        Ok(report) => report,
        Err(code) => return code,
    };

    print_delta(&base, &compare);
    ExitCode::SUCCESS
}

/// Converts validated reports into another tool's result format.
///
/// The conversion is deliberately lossy and says so on stderr: zkprof measures
/// time, and the target format also wants proof sizes, verification time and
/// security claims that no zkprof report contains. Those fields are omitted,
/// never zero-filled, because a zero in `proof_size` reads as "the proof is
/// empty" rather than "not measured".
fn export(args: &Export) -> ExitCode {
    let mut reports = Vec::with_capacity(args.reports.len());
    for path in &args.reports {
        match load_report(path) {
            Ok(report) => reports.push(report),
            Err(code) => return code,
        }
    }

    if args.format == ExportFormat::FibraceView {
        return export_fibrace(&reports, args);
    }

    let first = &reports[0];
    let target = match (&args.target, detect_target(&first.workload_id)) {
        (Some(explicit), _) => explicit.clone(),
        (None, Some(detected)) => detected.to_owned(),
        (None, None) => {
            eprintln!(
                "zkprof: workload {:?} does not name a canonical csp-benchmarks circuit; \
                 pass --target with one of {:?}",
                first.workload_id, CANONICAL_TARGETS
            );
            return ExitCode::FAILURE;
        }
    };

    let entries: Vec<(&Report, Option<u64>)> =
        reports.iter().map(|r| (r, args.input_size)).collect();
    let metadata = CspMetadata::new(rfc3339_utc(SystemTime::now()), &args.commit_sha);

    let document = match build_document(&args.system, &target, metadata, &entries) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("zkprof: {error}");
            return ExitCode::FAILURE;
        }
    };

    let json = match serde_json::to_string_pretty(&document) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("zkprof: could not serialise the document: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(path) = &args.out {
        if let Err(error) = std::fs::write(path, format!("{json}\n")) {
            eprintln!("zkprof: could not write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("zkprof: wrote {}", path.display());
    } else {
        println!("{json}");
    }

    eprintln!();
    eprintln!(
        "note: this document is an export, not a submission. {} field(s) that \
         csp-benchmarks expects are omitted because zkprof does not measure them:",
        UNMAPPABLE_FIELDS.len()
    );
    for (field, why) in UNMAPPABLE_FIELDS {
        eprintln!("  - {field}: {why}");
    }
    ExitCode::SUCCESS
}

/// Builds and prints a `fibrace-view` export of one or more reports.
///
/// A fibrace-view is the nearest-faithful mapping a zkprof report allows into
/// the FibRace (arXiv:2510.14693) result shape. Fields FibRace has that a
/// zkprof report does not measure are emitted as explicit `null` and explained
/// in `mapping_notes`; nothing is silently dropped and nothing is guessed. This
/// is an export, not a comparison: no performance claim is made about either
/// system.
fn export_fibrace(reports: &[Report], args: &Export) -> ExitCode {
    let views: Vec<FibraceView> = reports.iter().map(FibraceView::from_report).collect();
    let value = if views.len() == 1 {
        serde_json::to_value(&views[0])
    } else {
        serde_json::to_value(&views)
    };

    let json = match value {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("zkprof: could not serialise the fibrace-view: {error}");
                return ExitCode::FAILURE;
            }
        },
        Err(error) => {
            eprintln!("zkprof: could not build the fibrace-view: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(path) = &args.out {
        if let Err(error) = std::fs::write(path, format!("{json}\n")) {
            eprintln!("zkprof: could not write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("zkprof: wrote {}", path.display());
    } else {
        println!("{json}");
    }

    eprintln!();
    eprintln!(
        "note: this is a fibrace-view, not a submission. {} field(s) that FibRace \
         reports are null because a zkprof report does not measure them:",
        null_field_notes().len()
    );
    for note in null_field_notes() {
        eprintln!("  - {}: {}", note.field, note.why);
    }
    ExitCode::SUCCESS
}

/// Prints the same table `prove` prints, but from a saved document instead of a
/// fresh measurement. Per-operation figures divide the reported totals by the
/// report's own `iterations`, so the numbers match what `prove` would have
/// shown for that run.
fn print_report(report: &Report) {
    let iterations = report.iterations.max(1);
    let per_iteration = f64::from(iterations);

    println!("zkprof render — {}", report.workload_id);
    println!();
    println!(
        "  target      {} on {} ({})",
        report.target.kind, report.target.os, report.target.cpu
    );
    println!(
        "  system      {} over {}",
        report.system.proof_system, report.system.curve_or_field
    );
    if let Some(count) = report.system.constraint_count {
        println!("  circuit     {count} constraints");
    }
    println!(
        "  clock       {:.3} µs resolution, measured; cross-origin isolated: {}",
        report.timer_resolution_us, report.cross_origin_isolated
    );
    println!("  iterations  {iterations}");
    println!();
    println!("  {:<12} {:>10} {:>8}", "primitive", "ms/op", "share");

    for row in &report.primitives {
        println!(
            "  {:<12} {:>10.3} {:>7.1}%",
            row.name.as_str(),
            row.ms / per_iteration,
            row.share * 100.0
        );
    }

    println!("  {:<12} {:>10} {:>8}", "", "----------", "-------");
    println!(
        "  {:<12} {:>10.3} {:>7.1}%",
        "attributed",
        report.attributed_ms() / per_iteration,
        100.0
    );
    println!(
        "  {:<12} {:>10.3}",
        "unattributed",
        report.unattributed_ms() / per_iteration
    );
    println!(
        "  {:<12} {:>10.3}",
        "total",
        report.total_ms / per_iteration
    );
}

/// Prints the per-primitive delta between two reports.
///
/// Each row shows the `ms/op` of the comparand minus the baseline, the same
/// difference as a percentage of the baseline `ms/op`, and the change in share.
/// Rows present in one report but not the other are still shown — a missing
/// primitive is a real change, not something to hide. The convention is fixed
/// by construction: `Primitive::ALL` is the same declaration-order list every
/// other consumer uses.
fn print_delta(base: &Report, compare: &Report) {
    let iterations = base.iterations.max(1).max(compare.iterations.max(1));
    let per_iteration = f64::from(iterations);

    println!(
        "zkprof diff — {} vs {}",
        base.workload_id, compare.workload_id
    );
    println!();
    println!(
        "  {} ({} on {}) -> {} ({} on {})",
        base.target.kind,
        base.target.os,
        base.target.cpu,
        compare.target.kind,
        compare.target.os,
        compare.target.cpu
    );
    println!("  iterations  {iterations}");
    println!();
    println!(
        "  {:<12} {:>10} {:>10} {:>10}",
        "primitive", "Δ ms/op", "Δ %", "Δ share"
    );

    for primitive in Primitive::ALL {
        let base_row = base.primitive(primitive);
        let compare_row = compare.primitive(primitive);

        let base_ms = base_row.map_or(0.0, |row| row.ms / per_iteration);
        let compare_ms = compare_row.map_or(0.0, |row| row.ms / per_iteration);
        let delta_ms = compare_ms - base_ms;

        // Percentage change is undefined when the baseline is zero, which means
        // the primitive was not measured on either side or only appeared in the
        // comparand. Mark it rather than divide by zero.
        let delta_pct = if base_ms > 0.0 {
            format!("{:>9.1}%", (delta_ms / base_ms) * 100.0)
        } else if delta_ms == 0.0 {
            "      n/a".to_owned()
        } else {
            "   new".to_owned()
        };

        let base_share = base_row.map_or(0.0, |row| row.share);
        let compare_share = compare_row.map_or(0.0, |row| row.share);
        let delta_share = compare_share - base_share;

        let label = if base_row.is_none() {
            format!("{}+", primitive.as_str())
        } else if compare_row.is_none() {
            format!("{}-", primitive.as_str())
        } else {
            primitive.as_str().to_owned()
        };

        println!(
            "  {:<12} {:>10.3} {:>10} {:>9.1}pp",
            label,
            delta_ms,
            delta_pct,
            delta_share * 100.0
        );
    }

    let base_total = base.total_ms / per_iteration;
    let compare_total = compare.total_ms / per_iteration;
    let delta_total = compare_total - base_total;
    println!("  {:<12} {:>10} {:>10}", "", "----------", "-------");
    println!(
        "  {:<12} {:>10.3} {:>9.1}%",
        "total",
        delta_total,
        if base_total > 0.0 {
            (delta_total / base_total) * 100.0
        } else {
            0.0
        }
    );
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Prove(args) => prove(&args),
        Command::Render(args) => render(&args),
        Command::Diff(args) => diff(&args),
        Command::Export(args) => export(&args),
    }
}

fn prove(args: &Prove) -> ExitCode {
    if let Some(reason) = args.target.unimplemented_reason() {
        eprintln!(
            "zkprof: --target {} is not available: {reason}",
            args.target
        );
        return ExitCode::FAILURE;
    }
    let circuit = match args.circuit {
        CircuitArg::MerklePoseidon => {
            if args.depth == 0 {
                eprintln!("zkprof: --depth must be at least 1");
                return ExitCode::FAILURE;
            }
            Circuit::MerklePoseidon { depth: args.depth }
        }
        CircuitArg::Sha256 => {
            // Refusing a non-canonical size here rather than at export time:
            // a measurement at a size the exporter would reject is a
            // measurement that can never be published, and the run takes
            // minutes to find that out.
            let bytes = u64::try_from(args.bytes).unwrap_or(u64::MAX);
            if !SHA256_INPUT_SIZES.contains(&bytes) {
                eprintln!(
                    "zkprof: --bytes must be one of the csp-benchmarks sizes \
                     {SHA256_INPUT_SIZES:?}, got {}",
                    args.bytes
                );
                return ExitCode::FAILURE;
            }
            Circuit::Sha256 {
                input_bytes: args.bytes,
            }
        }
    };

    let workload_id = args
        .workload_id
        .clone()
        .unwrap_or_else(|| default_workload_id(&circuit));
    let config = zkprof_backend_arkworks::Config {
        circuit: circuit.clone(),
        seed: args.seed,
        workload_id,
        // The CLI runs where it was built. Only the browser harness knows a
        // browser, an OS and a CPU that the compiled code cannot see.
        target: Target::native(),
    };

    let outcome = match benchmark(&config) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("zkprof: {error}");
            let mut source = std::error::Error::source(&error);
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            return ExitCode::FAILURE;
        }
    };

    print_table(args, &circuit, &outcome);

    let json = match outcome.report.to_json_pretty() {
        Ok(json) => json,
        Err(error) => {
            eprintln!("zkprof: could not serialise the report: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = std::fs::write(&args.out, format!("{json}\n")) {
        eprintln!("zkprof: could not write {}: {error}", args.out.display());
        return ExitCode::FAILURE;
    }

    println!();
    println!("  report      {}", args.out.display());
    ExitCode::SUCCESS
}

fn print_table(args: &Prove, circuit: &Circuit, outcome: &Outcome) {
    let report = &outcome.report;
    let iterations = report.iterations.max(1);
    let per_iteration = f64::from(iterations);
    let below = outcome.below_target();

    println!("zkprof prove — {} on {}", args.backend, args.target);
    println!();
    println!("  workload    {}", report.workload_id);
    println!(
        "  circuit     {} constraints — {}",
        outcome.constraint_count,
        circuit.describe()
    );
    // "measured" because it is: the report's `timer_resolution_us` is probed
    // from the clock at run time, not read from a table of documented values.
    println!(
        "  clock       {:.3} µs resolution, measured; cross-origin isolated: {}",
        report.timer_resolution_us, report.cross_origin_isolated
    );
    println!(
        "  iterations  {} — {}",
        iterations,
        outcome.plan.scaling.describe()
    );
    println!();
    println!("  {:<12} {:>10} {:>8}", "primitive", "ms/op", "share");

    for row in &report.primitives {
        let flag = if below.contains(&row.name) {
            "  <- below target"
        } else {
            ""
        };
        println!(
            "  {:<12} {:>10.3} {:>7.1}%{}",
            row.name.as_str(),
            row.ms / per_iteration,
            row.share * 100.0,
            flag
        );
    }

    println!("  {:<12} {:>10} {:>8}", "", "----------", "-------");
    println!(
        "  {:<12} {:>10.3} {:>7.1}%",
        "attributed",
        report.attributed_ms() / per_iteration,
        100.0
    );
    println!(
        "  {:<12} {:>10.3}",
        "unattributed",
        report.unattributed_ms() / per_iteration
    );
    println!(
        "  {:<12} {:>10.3}",
        "total",
        report.total_ms / per_iteration
    );

    if !below.is_empty() {
        println!();
        println!(
            "  note: {} row(s) are under {TARGET_RATIO:.0}x the clock resolution. \
             They are measured, but at the noise floor — do not compare them.",
            below.len()
        );
    }
}
