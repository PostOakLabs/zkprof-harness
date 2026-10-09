// SPDX-License-Identifier: Apache-2.0
//
// Task 1 — Oracle dump. Decodes each estate receipt fixture into the raw
// primitives the snarkjs-style independent JS twin needs to reproduce the
// verification WITHOUT running risc0's verifier:
//
//   * image_id            — the sha256 image digest the claim binds.
//   * journal_digest      — risc0 sha256 of the RFC8785/JCS journal bytes.
//   * claim_digest        — the `ReceiptClaim::ok(...).digest()`.
//   * control_root        — `VerifierContext::default()` groth16 params.
//   * bn254_control_id    — same.
//   * public_inputs       — the 5 Groth16 public inputs (CORRECT order:
//                           [a0..a1 from control_root, c0..c1 from claim,
//                            id from the reversed control_id], as BigInt
//                           decimal strings).
//   * proof A/B/C         — the seal decoded into affine coordinates
//                           (noble-friendly: G1 {x,y}, G2 {x_c0,x_c1,y_c0,y_c1}).
//   * verifying_key       — alpha/beta/gamma/delta/ic[0..5] affine
//                           coordinates.
//
// Every point is emitted as coordinate values in the SAME (c0, c1) extension
// basis both ark-bn254 and the noble bundle use (verified by identical
// generator/b constants). G1 coords and G2 c0/c1 are decimal strings.
//
// Usage: cargo run -p zkprof-verify --bin dump [PATH...] [--out-dir DIR]
//   no args -> fixtures/compute-proof/*.json, writes fixtures/twin-oracle/*.json
//   explicit paths -> same receipts, same output location.
//   --out-dir DIR -> write the oracles to DIR instead (used by the corpus sweep
//                    to keep derived oracles out of the committed fixtures).
// Exit 0.

use std::path::{Path, PathBuf};

use ark_ff::PrimeField;
use ark_serialize::CanonicalDeserialize;
use base64::Engine;
use risc0_binfmt::Digestible;
use risc0_zkvm::sha::Impl as ShaImpl;
use risc0_zkvm::{Digest, MaybePruned, ReceiptClaim, VerifierContext};
use serde_json::{json, Value};

/// BN254 scalar field modulus r (order of both G1 and G2).
const BN254_R: &str =
    "21888242871839275222246405745257275088548364400416034343698204186575808495617";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // Optional trailing output dir: `cargo run --bin dump [receipt...] --out-dir DIR`.
    // When set, the oracles are written there (used by the corpus sweep so the
    // derived oracles live outside the committed twin-oracle fixtures). The
    // default output dir is unchanged (`fixtures/twin-oracle`).
    let mut out_dir_override: Option<PathBuf> = None;
    if let Some(pos) = args.iter().position(|a| a == "--out-dir") {
        out_dir_override = Some(PathBuf::from(args[pos + 1].clone()));
        args.drain(pos..pos + 2);
    }

    let mut paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
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

    let ctx = VerifierContext::default();
    let params = ctx
        .groth16_verifier_parameters
        .as_ref()
        .expect("groth16 verifier parameters present in default context");

    // Params digest sanity: must equal the pinned stable value in risc0-zkvm
    // tests. Confirms our understanding of the constant set + hashing.
    let params_digest = params.digest::<ShaImpl>();
    let params_digest_hex = hex::encode(params_digest.as_bytes());
    eprintln!("[dump] verifier-parameters digest = sha256:{params_digest_hex}");

    let control_root_hex = hex::encode(params.control_root.as_bytes());
    let control_id_hex = hex::encode(params.bn254_control_id.as_bytes());

    // Decode the verifying key (risc0_groth16::VerifyingKey) into arkworks. The
    // inner ark field is pub(crate) to risc0-groth16, so we round-trip through
    // its serde serialization (a raw CanonicalSerialize byte buffer) and
    // deserialize with ark-groth16 to reach the pub alpha/beta/gamma/delta/IC.
    let vk_bytes = serde_json::to_value(&params.verifying_key).expect("serialize verifying key");
    let vk_bytes: Vec<u8> = vk_bytes
        .as_array()
        .expect("verifying key serializes as byte array")
        .iter()
        .map(|v| v.as_u64().expect("byte value") as u8)
        .collect();
    let ark_vk =
        ark_groth16::VerifyingKey::<ark_bn254::Bn254>::deserialize_uncompressed(&vk_bytes[..])
            .expect("verify key deserializes");

    // Precompute the 5 public inputs once (they are fixture-independent except
    // c0/c1 which depend on the claim digest).
    let (a0, a1) = split_digest(&params.control_root);
    let id = control_id_fr(&params.bn254_control_id);

    let out_dir = out_dir_override
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/twin-oracle"));
    std::fs::create_dir_all(&out_dir).expect("create oracle out dir");

    for path in &paths {
        let text = std::fs::read_to_string(path).expect("read receipt");
        let receipt: Value = serde_json::from_str(&text).expect("parse receipt");
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(".receipt.json", "");

        let journal = receipt.get("journal").expect("journal present");
        let image_id = receipt
            .get("imageId")
            .and_then(|v| v.as_str())
            .expect("imageId");

        let image_hex = image_id.strip_prefix("sha256:").expect("sha256: imageId");
        let image_bytes: [u8; 32] = hex::decode(image_hex)
            .expect("imageId hex")
            .try_into()
            .expect("32 bytes");
        let image_digest = Digest::from(image_bytes);

        // JCS bytes and their risc0 sha256 journal digest.
        let jcs = zkprof_verify::canonical_jcs(journal);
        let journal_digest_hex = zkprof_verify::journal_digest_hex(&jcs);

        // ReceiptClaim::ok(...).digest()
        let claim = ReceiptClaim::ok(image_digest, MaybePruned::Value(jcs));
        let claim_digest = claim.digest::<ShaImpl>();
        let claim_digest_hex = hex::encode(claim_digest.as_bytes());

        let (c0, c1) = split_digest(&claim_digest);

        // Decode the seal into A (G1), B (G2), C (G1) affine coordinates.
        let seal_b64 = receipt.get("seal").and_then(|v| v.as_str()).expect("seal");
        let seal = base64::engine::general_purpose::STANDARD
            .decode(seal_b64)
            .expect("seal base64");
        let (pa, pb, pc) = decode_seal(&seal);

        let public_inputs = [a0.clone(), a1.clone(), c0.clone(), c1.clone(), id.clone()];

        let out = json!({
            "source": path.file_name().unwrap().to_string_lossy(),
            "image_id": image_id,
            "image_id_hex": image_hex,
            "journal_digest": journal_digest_hex.clone(),
            "journal_digest_hex": journal_digest_hex,
            "claim_digest": claim_digest_hex,
            "control_root": control_root_hex,
            "bn254_control_id": control_id_hex,
            "public_inputs": public_inputs.clone(),
            "proof": {
                "A": pa,
                "B": pb,
                "C": pc,
            },
            "verifying_key": {
                "alpha": g1(&ark_vk.alpha_g1),
                "beta": g2(&ark_vk.beta_g2),
                "gamma": g2(&ark_vk.gamma_g2),
                "delta": g2(&ark_vk.delta_g2),
                "ic": ark_vk.gamma_abc_g1.iter().map(g1).collect::<Vec<_>>(),
            },
        });

        let out_path = out_dir.join(format!("{name}.oracle.json"));
        std::fs::write(&out_path, serde_json::to_string_pretty(&out).unwrap())
            .expect("write oracle");
        eprintln!("[dump] wrote {}", out_path.display());
        eprintln!("[dump]   {name}: journal_digest={journal_digest_hex}");
        eprintln!("[dump]   {name}: claim_digest={claim_digest_hex}");
        eprintln!(
            "[dump]   {name}: public_inputs=[{}]",
            public_inputs.join(", ")
        );
    }
}

/// risc0 `split_digest`: reverse the digest; a0 = Fr(be[16..32]), a1 = Fr(be[0..16]).
/// Each 128-bit half is already < r, so no mod needed (we emit the bigint).
fn split_digest(d: &Digest) -> (String, String) {
    let bytes = d.as_bytes();
    let mut be = bytes.to_vec();
    be.reverse(); // big_endian
    let (lo, hi) = be.split_at(be.len() / 2); // lo = be[0..16], hi = be[16..32]
                                              // a1 = Fr(lo), a0 = Fr(hi)  (risc0 returns (a0, a1) = (fr(hi), fr(lo)))
    let a0 = bigint_be(hi).to_string();
    let a1 = bigint_be(lo).to_string();
    (a0, a1)
}

/// The 5th public input: fr of the reversed control_id, reduced mod r.
/// Exactly mirrors risc0's `fr_from_bytes(rev(control_id))` == LE(control_id) mod r.
fn control_id_fr(control_id: &Digest) -> String {
    let bytes = control_id.as_bytes();
    let mut le = num_bigint::BigUint::from(0u8);
    for (i, b) in bytes.iter().enumerate() {
        le += num_bigint::BigUint::from(*b) << (8 * i);
    }
    let r: num_bigint::BigUint = BN254_R.parse().unwrap();
    (le % &r).to_string()
}

fn bigint_be(bytes: &[u8]) -> num_bigint::BigUint {
    let mut acc = num_bigint::BigUint::from(0u8);
    for b in bytes {
        acc = (acc << 8) | num_bigint::BigUint::from(*b);
    }
    acc
}

/// G1 affine -> {x, y} decimal strings.
fn g1(p: &ark_bn254::G1Affine) -> Value {
    json!({
        "x": p.x.into_bigint().to_string(),
        "y": p.y.into_bigint().to_string(),
    })
}

/// G2 affine -> {x_c0, x_c1, y_c0, y_c1} decimal strings.
fn g2(p: &ark_bn254::G2Affine) -> Value {
    json!({
        "x_c0": p.x.c0.into_bigint().to_string(),
        "x_c1": p.x.c1.into_bigint().to_string(),
        "y_c0": p.y.c0.into_bigint().to_string(),
        "y_c1": p.y.c1.into_bigint().to_string(),
    })
}

/// Decode the 256-byte seal into A (G1), B (G2), C (G1) using arkworks
/// deserialization, exactly as risc0's `g1_from_bytes` / `g2_from_bytes` do.
/// Returns A, B, C as JSON coordinate objects.
fn decode_seal(seal: &[u8]) -> (Value, Value, Value) {
    const E: usize = 32;
    const G1: usize = E * 2;
    const G2: usize = E * 4;
    assert_eq!(seal.len(), 2 * G1 + G2, "seal must be 256 bytes");

    // A: bytes[0..64] as x(BE) ++ y(BE)
    let a_stream = rev(&seal[0..E]);
    let a_stream = concat(&a_stream, &rev(&seal[E..G1]));
    let a_pt = ark_bn254::G1Affine::deserialize_uncompressed(&a_stream[..]).expect("A on curve");

    // C: bytes[192..256]
    let c_stream = rev(&seal[G1 + G2..G1 + G2 + E]);
    let c_stream = concat(&c_stream, &rev(&seal[G1 + G2 + E..2 * G1 + G2]));
    let c_pt = ark_bn254::G1Affine::deserialize_uncompressed(&c_stream[..]).expect("C on curve");

    // B: 4 x 32 bytes; ark order = x_c0, x_c1, y_c0, y_c1 (LE). The seal's 256
    // bytes hold b as [ x_c1, x_c0, y_c1, y_c0 ] (each BE) at offsets 64..192, so
    // ark stream = rev(x_c0) ++ rev(x_c1) ++ rev(y_c0) ++ rev(y_c1)
    //             = rev(bx1)  ++ rev(bx0)  ++ rev(by1)  ++ rev(by0).
    let b = &seal[G1..G1 + G2];
    let (bx0, bx1, by0, by1) = (&b[0..E], &b[E..2 * E], &b[2 * E..3 * E], &b[3 * E..4 * E]);
    let b_stream = concat(&concat(&rev(bx1), &rev(bx0)), &rev(by1));
    let b_stream = concat(&b_stream, &rev(by0));
    let b_pt = ark_bn254::G2Affine::deserialize_uncompressed(&b_stream[..]).expect("B on curve");

    (g1(&a_pt), g2(&b_pt), g1(&c_pt))
}

fn rev(b: &[u8]) -> Vec<u8> {
    let mut v = b.to_vec();
    v.reverse();
    v
}

fn concat(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut v = a.to_vec();
    v.extend_from_slice(b);
    v
}
