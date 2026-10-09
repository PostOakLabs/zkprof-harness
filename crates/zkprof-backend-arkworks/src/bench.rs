// SPDX-License-Identifier: Apache-2.0

//! Runs the demo workload and turns the result into a report.
//!
//! # What is measured, and what is not
//!
//! The trusted setup is *not* measured. It is a one-off per circuit, and
//! including it would make `total_ms` a property of the setup rather than of
//! the workload being compared. The proof-of-correctness pairing check is not
//! measured either.
//!
//! Both run before the profiler is constructed, which is what keeps them out
//! of `total_ms`: a [`Profiler`] starts its region when it is built, so
//! anything that happens before that simply is not in the report.
//!
//! # The memory figure
//!
//! On `wasm32` the report carries `memory.peak_heap_bytes`, read from the
//! size of the module's linear memory *after* the measured region. That is a
//! high-water mark for the module, not for the workload: it includes
//! everything allocated before the run started. See
//! [`zkprof_core::linear_memory_bytes`] for why this is the only memory
//! figure a wasm module can read about itself. Off `wasm32` the field is
//! absent rather than zero.
//!
//! # Iteration scaling
//!
//! The iteration count comes from [`zkprof_core::scale::plan_iterations`],
//! which measures a batch and escalates until the run is at least
//! [`zkprof_core::scale::TARGET_RATIO`] times the measured clock resolution.
//! It plans on the *whole* proof, not on each primitive, so a primitive that
//! is a small fraction of a proof can still land below the target.
//! `Outcome::plan` carries the result and [`Outcome::below_target`] reports
//! exactly which rows it affects, so the caller can say so rather than
//! quietly publishing a number measured at the clock's noise floor.

use std::cell::RefCell;

use ark_bn254::{Bn254, Fr};
use ark_crypto_primitives::snark::SNARK;
use ark_ff::ToConstraintField;
use ark_groth16::r1cs_to_qap::LibsnarkReduction;
use ark_groth16::Groth16;
use ark_relations::r1cs::{ConstraintSynthesizer, SynthesisError};
use ark_std::rand::rngs::StdRng;
use ark_std::rand::Rng;
use ark_std::rand::SeedableRng;
use ark_std::UniformRand;
use zkprof_core::clock::{platform_clock, ClockError};
use zkprof_core::linear_memory_bytes;
use zkprof_core::model::{Memory, Primitive, Report, ReportError, System, Target};
use zkprof_core::profiler::Profiler;
use zkprof_core::scale::{plan_iterations, Plan, TARGET_RATIO};

use crate::circuit::{membership_witness, poseidon_config, MerklePoseidonCircuit, DEFAULT_DEPTH};
use crate::prover::prove_with_spans;
use crate::sha256::{sha256_digest, Sha256PreimageCircuit, DIGEST_BYTES};

/// Seed for the witness, the setup and the prover randomness.
///
/// Fixed so that two runs of the same configuration are comparable. There is
/// nothing secret here and the setup is not a real ceremony.
pub const DEFAULT_SEED: u64 = 0x5EED;

/// The circuit a run proves. This *is* the workload.
///
/// Two circuits are here for opposite reasons. [`Circuit::MerklePoseidon`] is
/// this repository's own shape — the one the harness was built around, and
/// the one every number collected before W3.8 was measured on.
/// [`Circuit::Sha256`] is a canonical circuit from csp-benchmarks, and it is
/// here so that a number measured in this repository can be *compared*: run
/// the same circuit natively and in a browser and the cross-target delta is
/// one row of a table, which is the whole thesis. A delta on a circuit nobody
/// else benchmarks is a number that means nothing outside this repository.
///
/// Only circuits the arkworks backend can actually express belong here. The
/// ones it cannot are left out rather than approximated — a SHA-256
/// that is nearly the canonical SHA-256 is not the canonical SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Circuit {
    /// Depth-parameterised Merkle-Poseidon membership.
    MerklePoseidon {
        /// One Poseidon permutation per level, so this is the circuit-size
        /// knob.
        depth: usize,
    },

    /// Knowledge of a preimage of a public SHA-256 digest.
    Sha256 {
        /// Preimage length in bytes, and therefore the circuit-size knob.
        /// Sizes csp-benchmarks measures are in
        /// [`crate::sha256::SHA256_INPUT_SIZES`].
        input_bytes: usize,
    },
}

impl Circuit {
    /// The `workload_id` this circuit is recorded under.
    ///
    /// For [`Circuit::Sha256`] the circuit name must be the *leading* token,
    /// because [`zkprof_core::csp::detect_target`] resolves the circuit from
    /// the front of the id and refuses anything it cannot place. `sha256-128`
    /// resolves; `groth16-bn254-sha256-128` would not, and the export would
    /// then need a human to name the target by hand.
    #[must_use]
    pub fn workload_id(&self) -> String {
        match self {
            Self::MerklePoseidon { depth } => format!("groth16-bn254-merkle-poseidon-d{depth}"),
            Self::Sha256 { input_bytes } => format!("sha256-{input_bytes}"),
        }
    }

    /// One line naming the circuit and its size, for the run header.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::MerklePoseidon { depth } => format!("merkle-poseidon, depth {depth}"),
            Self::Sha256 { input_bytes } => format!("sha256 preimage, {input_bytes} bytes"),
        }
    }
}

impl Default for Circuit {
    fn default() -> Self {
        Self::MerklePoseidon {
            depth: DEFAULT_DEPTH,
        }
    }
}

/// What a run needs, beyond the defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The circuit to prove, and therefore the workload to measure.
    pub circuit: Circuit,

    /// Seed for every random draw in the run.
    pub seed: u64,

    /// `workload_id` written into the report.
    pub workload_id: String,

    /// Where the workload ran.
    ///
    /// Not derived from the build target, because the two are not the same
    /// question: a wasm build reports a browser, an OS and a CPU the Rust code
    /// cannot see, and only the caller knows them. The native default is what
    /// [`crate::bench::benchmark`] used to hard-code.
    pub target: Target,
}

impl Default for Config {
    fn default() -> Self {
        let circuit = Circuit::default();
        Self {
            workload_id: circuit.workload_id(),
            circuit,
            seed: DEFAULT_SEED,
            target: Target::native(),
        }
    }
}

impl Config {
    /// A config for a different circuit, keeping the default seed.
    #[must_use]
    pub fn with_circuit(circuit: Circuit) -> Self {
        Self {
            workload_id: circuit.workload_id(),
            circuit,
            ..Self::default()
        }
    }

    /// A config with a different Merkle depth, keeping the default seed.
    #[must_use]
    pub fn with_depth(depth: usize) -> Self {
        Self::with_circuit(Circuit::MerklePoseidon { depth })
    }

    /// A config for a different target, keeping the rest of the defaults.
    #[must_use]
    pub fn with_target(target: Target) -> Self {
        Self {
            target,
            ..Self::default()
        }
    }
}

/// The `workload_id` for a circuit.
///
/// The size is in the id because it *is* the workload: a depth-8 report and a
/// depth-64 report are not measurements of the same thing, and a `diff`
/// between them is meaningless. Same for a 128-byte and a 2048-byte preimage.
#[must_use]
pub fn default_workload_id(circuit: &Circuit) -> String {
    circuit.workload_id()
}

/// The result of one benchmark run.
#[derive(Debug)]
pub struct Outcome {
    /// The schema-valid report.
    pub report: Report,

    /// Constraints in the circuit, measured during synthesis rather than
    /// predicted from the depth.
    pub constraint_count: u64,

    /// How the iteration count was chosen, including whether the target was
    /// met.
    pub plan: Plan,

    /// The public inputs the proof is verified against.
    ///
    /// One element for a Merkle root; more than one for a SHA-256 digest,
    /// because the circuit packs the 32 bytes into field elements before
    /// allocating them as the instance. Kept on the outcome so a caller can
    /// re-verify independently rather than taking the pipeline's word for it.
    pub public_inputs: Vec<Fr>,
}

impl Outcome {
    /// Primitive rows whose accumulated time is under [`TARGET_RATIO`] times
    /// the measured clock resolution.
    ///
    /// A non-empty result is not an error, but it is a reason not to compare
    /// these rows against anything.
    #[must_use]
    pub fn below_target(&self) -> Vec<Primitive> {
        let floor_us =
            TARGET_RATIO * self.report.timer_resolution_us * f64::from(self.report.iterations);
        self.report
            .primitives
            .iter()
            .filter(|row| row.ms * 1_000.0 < floor_us)
            .map(|row| row.name)
            .collect()
    }
}

/// Why a run failed.
#[derive(Debug)]
pub enum Error {
    /// No usable clock on this platform.
    Clock(ClockError),
    /// Synthesis, setup, or the QAP witness map failed.
    Synthesis(SynthesisError),
    /// The assembled report failed validation.
    Report(ReportError),
    /// The span-split prover produced a proof that does not verify.
    ProofDidNotVerify,
    /// The digest could not be packed into the circuit's public inputs.
    ///
    /// Only reachable for [`Circuit::Sha256`], and only if the field cannot
    /// hold the packed bytes. Failing loudly here beats verifying against a
    /// silently truncated instance.
    DigestEncoding,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clock(error) => write!(f, "no usable clock: {error}"),
            Self::Synthesis(error) => write!(f, "prover failed: {error}"),
            Self::Report(error) => write!(f, "report failed validation: {error}"),
            Self::ProofDidNotVerify => f.write_str(
                "the span-split prover produced a proof that does not verify; \
                 do not trust this report",
            ),
            Self::DigestEncoding => f.write_str(
                "the SHA-256 digest could not be packed into the circuit's public inputs",
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Clock(error) => Some(error),
            Self::Synthesis(error) => Some(error),
            Self::Report(error) => Some(error),
            Self::ProofDidNotVerify | Self::DigestEncoding => None,
        }
    }
}

impl From<ClockError> for Error {
    fn from(error: ClockError) -> Self {
        Self::Clock(error)
    }
}

impl From<SynthesisError> for Error {
    fn from(error: SynthesisError) -> Self {
        Self::Synthesis(error)
    }
}

impl From<ReportError> for Error {
    fn from(error: ReportError) -> Self {
        Self::Report(error)
    }
}

/// Proves the configured circuit repeatedly and reports where the time went.
///
/// The circuit is [`Config::circuit`]. Both circuits run through exactly the
/// same timing harness below — that is the point of the split. Only the
/// witness and the public inputs differ between them, so a cross-target delta
/// measured on the canonical circuit is a delta on the same instrument, not
/// two instruments that happen to share a name.
///
/// # Errors
///
/// - [`Error::Clock`] if the platform clock cannot be opened.
/// - [`Error::Synthesis`] if setup, synthesis or the QAP reduction fails.
/// - [`Error::ProofDidNotVerify`] if the pipeline's own proof does not verify
///   — the report is withheld in that case, because a broken prover produces
///   a beautifully formatted measurement of the wrong thing.
/// - [`Error::DigestEncoding`] if a SHA-256 digest will not pack into field
///   elements as public inputs.
/// - [`Error::Report`] if the assembled report fails schema validation.
pub fn benchmark(config: &Config) -> Result<Outcome, Error> {
    let mut rng = StdRng::seed_from_u64(config.seed);

    match config.circuit {
        Circuit::MerklePoseidon { depth } => {
            let params = poseidon_config::<Fr>();
            let leaf = Fr::rand(&mut rng);
            let (root, siblings, positions) = membership_witness(&params, leaf, depth, &mut rng);
            let witnessed = MerklePoseidonCircuit::blank(params.clone(), depth)
                .with_witness(root, leaf, siblings, positions);
            run(
                config,
                &mut rng,
                &witnessed,
                MerklePoseidonCircuit::blank(params, depth),
                vec![root],
            )
        }
        Circuit::Sha256 { input_bytes } => {
            // The preimage is only a witness, so it only has to be
            // deterministic under the seed: there is no "right" preimage, and
            // any bytes of the correct length give a circuit with the same
            // shape and the same cost.
            let preimage: Vec<u8> = (0..input_bytes).map(|_| rng.gen::<u8>()).collect();
            let digest = sha256_digest(&preimage);
            let witnessed =
                Sha256PreimageCircuit::blank(input_bytes).with_witness(digest, preimage);

            let public_inputs = digest_public_inputs(&digest).ok_or(Error::DigestEncoding)?;

            run(
                config,
                &mut rng,
                &witnessed,
                Sha256PreimageCircuit::blank(input_bytes),
                public_inputs,
            )
        }
    }
}

/// The public inputs a SHA-256 preimage proof is verified against.
///
/// These come from the *same* call the circuit uses, not from a re-derivation
/// of the packing: inside the circuit, [`UInt8::new_input_vec`] packs the 32
/// digest bytes into field elements and allocates those as the instance. Two
/// implementations of that packing is one implementation too many, so this
/// function exists mainly to give that call a name and a place to explain
/// itself. `tests::the_sha256_workload_proves_and_verifies` is what actually
/// stops the two sides drifting apart — if the encodings disagree, the proof
/// does not verify and the run is refused.
///
/// [`UInt8::new_input_vec`]: ark_r1cs_std::uint8::UInt8::new_input_vec
#[must_use]
pub fn digest_public_inputs(digest: &[u8; DIGEST_BYTES]) -> Option<Vec<Fr>> {
    digest.to_field_elements()
}

/// The timing harness, generic over which circuit is being proved.
///
/// `blank` is synthesised for key generation and `circuit` is proved; they
/// must be the same circuit with and without its witness, because Groth16
/// setup and proving have to agree on the constraint system exactly.
fn run<C>(
    config: &Config,
    rng: &mut StdRng,
    circuit: &C,
    blank: C,
    public_inputs: Vec<Fr>,
) -> Result<Outcome, Error>
where
    C: ConstraintSynthesizer<Fr> + Clone,
{
    let clock = platform_clock()?;

    let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(blank, rng)?;
    let (r, s) = (Fr::rand(rng), Fr::rand(rng));

    // A profiler used only to size the workload. Its timings are discarded:
    // `plan_iterations` needs a closure that proves, and proving needs a
    // profiler to accumulate into, but the profiler that matters must not
    // start its region until the plan is settled.
    let scratch = Profiler::new(
        clock.as_ref(),
        config.target.clone(),
        System::new("groth16", "bn254"),
        "planning",
    );
    let failure: RefCell<Option<SynthesisError>> = RefCell::new(None);
    let mut workload =
        || match prove_with_spans::<Bn254, LibsnarkReduction, _>(circuit, &pk, r, s, &scratch) {
            Ok(_) => {}
            Err(error) => *failure.borrow_mut() = Some(error),
        };
    let plan = plan_iterations(clock.as_ref(), &mut workload);
    if let Some(error) = failure.take() {
        return Err(error.into());
    }

    // Correctness gate, outside the measured region.
    let proved = prove_with_spans::<Bn254, LibsnarkReduction, _>(circuit, &pk, r, s, &scratch)?;
    let pvk = Groth16::<Bn254>::process_vk(&vk)?;
    if !Groth16::<Bn254>::verify_proof(&pvk, &proved.proof, &public_inputs)? {
        return Err(Error::ProofDidNotVerify);
    }
    let constraint_count = u64::try_from(proved.constraint_count).unwrap_or(u64::MAX);

    let profiler = Profiler::new(
        clock.as_ref(),
        config.target.clone(),
        System::new("groth16", "bn254").with_constraint_count(constraint_count),
        config.workload_id.clone(),
    )
    .with_iterations(plan.scaling.iterations)
    .with_backend_id("arkworks-groth16")
    .with_version(zkprof_core::REPORT_VERSION)
    .with_commitment_scheme("groth16");

    for _ in 0..plan.scaling.iterations {
        prove_with_spans::<Bn254, LibsnarkReduction, _>(circuit, &pk, r, s, &profiler)?;
    }

    // Read after the workload: a high-water mark is only observable at the
    // end. Off wasm the probe returns None and the field stays absent rather
    // than reporting 0.
    if let Some(bytes) = linear_memory_bytes() {
        profiler.record_memory(Memory::peak_heap_bytes(bytes));
    }

    let report = profiler.finish()?;

    Ok(Outcome {
        report,
        constraint_count,
        plan,
        public_inputs,
    })
}
