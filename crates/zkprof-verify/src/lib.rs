// SPDX-License-Identifier: Apache-2.0

//! Client-side verifier for risc0 `groth16-bn254` receipts (route A).
//!
//! This crate verifies a receipt's Groth16/BN254 seal against a claim derived
//! from an image ID and a journal digest, using the pinned `risc0-zkvm` 3.0.5
//! verifier (the version the AINumbers receipts claim). Every binding check is
//! named individually so a failure names *which* binding broke: seal valid /
//! imageId expected / receipt type-system-format gate.
//!
//! The seal-valid check is the single cryptographic binding: a valid Groth16
//! seal by construction ties the claim (image id + journal digest) to the
//! proof. There is no separate "claim digest == journal digest" row — that
//! would re-print the seal result, and it can never disagree with it.
//!
//! # The journal-bytes hypothesis
//!
//! The estate `ZkVmReceipt` stores `journal` as decoded JSON, not committed
//! bytes, so the journal digest is unknowable unless we guess the byte
//! serialization the guest committed. `verify()` verifies against the primary
//! hypothesis (RFC 8785/JCS-canonical bytes of the JSON object); the obvious
//! alternates (compact, pretty) are enumerated by `journal_candidate_bytes` for
//! the per-receipt binary. `docs/JOURNAL-BYTES-HYPOTHESIS.md` records the
//! per-receipt result.
//!
//! Honesty: a verifier that cannot fail is worse than one that reports
//! `NOT-VERIFIED`. If a receipt-seal semantic cannot be reproduced the crate
//! reports the exact step it could not perform rather than claiming a pass.

use std::collections::BTreeMap;

use base64::Engine;
use risc0_binfmt::Digestible;
use risc0_zkvm::sha::Impl as ShaImpl;
use risc0_zkvm::{
    Digest, Groth16Receipt, InnerReceipt, Journal, MaybePruned, Receipt, ReceiptClaim,
    VerifierContext,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One named binding check. `passed = None` means the check was NOT run (not a
/// pass, not a fail) — the honest third state for a step we could not perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: Option<bool>,
    pub detail: String,
    /// Wall ms spent on this check (single machine; measurement, not benchmark).
    pub ms: f64,
}

/// The result of verifying one receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub valid: bool,
    pub image_id: Option<String>,
    pub journal_digest: Option<String>,
    pub journal: Value,
    pub checks: Vec<Check>,
}

/// A single journal-bytes candidate digest, with the hypothesis it tests.
#[derive(Debug, Clone, Serialize)]
pub struct JournalCandidate {
    pub label: String,
    pub bytes_hex_prefix: String,
    pub digest: String,
}

/// Verify a receipt JSON document.
///
/// `expected_image_id` optional: when supplied, the imageId-recognized/expected
/// checks compare against it; when absent, imageId is only checked for shape.
pub fn verify(receipt_json: &str, expected_image_id: Option<&str>) -> VerifyResult {
    let parse = serde_json::from_str::<Value>(receipt_json);
    let mut checks: Vec<Check> = Vec::new();
    let mut journal = Value::Null;
    let image_id = None;
    let mut journal_digest = None;
    let mut expected_ok = true;
    if let Err(e) = parse {
        checks.push(Check {
            name: "receipt parses as JSON".into(),
            passed: Some(false),
            detail: format!("{e}"),
            ms: 0.0,
        });
        return VerifyResult {
            valid: false,
            image_id,
            journal_digest,
            journal,
            checks,
        };
    }
    let receipt = parse.unwrap();

    // --- structural: seal parse / imageId shape / journal present ------------
    let seal = receipt.get("seal").and_then(|v| v.as_str());
    let seal_decoded = seal.map(|s| base64::engine::general_purpose::STANDARD.decode(s));
    let seal_bytes = match seal_decoded {
        Some(Ok(bytes)) => Some(bytes),
        _ => None,
    };
    checks.push(Check {
        name: "seal decodes as base64".into(),
        passed: Some(seal_bytes.is_some()),
        detail: match &seal_bytes {
            Some(bytes) => format!("{} decoded bytes", bytes.len()),
            None => match seal {
                Some(_) => "base64 decode failed".to_string(),
                None => "no seal field".to_string(),
            },
        },
        ms: 0.0,
    });

    let image_id = receipt
        .get("imageId")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let image_id_shape_ok = image_id
        .as_deref()
        .map(|s| {
            s.strip_prefix("sha256:")
                .map(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
                .unwrap_or(false)
        })
        .unwrap_or(false);
    checks.push(Check {
        name: "imageId is sha256:hex".into(),
        passed: Some(image_id_shape_ok),
        detail: image_id.clone().unwrap_or_else(|| "(none)".to_string()),
        ms: 0.0,
    });

    journal = receipt.get("journal").cloned().unwrap_or(Value::Null);
    checks.push(Check {
        name: "journal present".into(),
        passed: Some(journal.is_object()),
        detail: if journal.is_object() {
            "object".to_string()
        } else {
            "not an object".to_string()
        },
        ms: 0.0,
    });

    if let Some(expected) = expected_image_id {
        let matched = image_id.as_deref() == Some(expected);
        expected_ok = matched;
        checks.push(Check {
            name: "imageId expected".into(),
            passed: Some(matched),
            detail: format!(
                "{} vs expected {}",
                image_id.clone().unwrap_or_default(),
                expected
            ),
            ms: 0.0,
        });
    }

    // --- format/version gate: the verifier hardcodes Groth16/BN254 ----------
    // A receipt declaring a different `system`, `receiptFormat` or `type` must
    // not be silently verified under the current parameters. Read all three and
    // stop before crypto when they are not what this crate knows how to check.
    let declared_system = receipt.get("system").and_then(|v| v.as_str()).unwrap_or("");
    let declared_receipt_format = receipt
        .get("receiptFormat")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let declared_type = receipt.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let format_ok = declared_system == "risc0"
        && declared_receipt_format == "groth16-bn254"
        && declared_type == "ZkVmReceipt";
    checks.push(Check {
        name: "receipt type/system/receiptFormat supported".into(),
        passed: Some(format_ok),
        detail: format!(
            "system={declared_system:?} receiptFormat={declared_receipt_format:?} type={declared_type:?}"
        ),
        ms: 0.0,
    });
    if !format_ok {
        return VerifyResult {
            valid: false,
            image_id,
            journal_digest,
            journal,
            checks,
        };
    }

    // If any structural check failed hard, stop before attempting crypto.
    if !(seal_bytes.is_some() && journal.is_object()) {
        return VerifyResult {
            valid: false,
            image_id,
            journal_digest,
            journal,
            checks,
        };
    }

    // --- journal-bytes hypothesis: verify with JCS bytes --------------------
    let jcs_bytes = canonical_jcs(&journal);
    let jcs_digest = digest_of(&jcs_bytes);
    journal_digest = Some(format!("sha256:{}", hex::encode(jcs_digest.as_bytes())));

    let seal_bytes = seal_bytes.unwrap();
    let result = run_risc0_verify(&seal_bytes, &jcs_bytes, image_id.as_deref());
    let seal_ok_run = result.is_ok();
    checks.push(Check {
        name: "seal valid (Groth16 over BN254)".into(),
        passed: Some(seal_ok_run),
        detail: match &result {
            Ok(()) => "verified".to_string(),
            Err(e) => e.clone(),
        },
        ms: 0.0,
    });

    VerifyResult {
        valid: seal_ok_run && expected_ok,
        image_id,
        journal_digest,
        journal,
        checks,
    }
}

/// Run the risc0 3.0.5 verifier: build a `Receipt` whose journal is `journal_bytes`
/// and whose claim binds `image_id` to the journal digest, then `verify`.
///
/// Returns `Ok(())` on full verification, or a description of the exact step
/// that blocked/failed (the honest register: a semantic we could not reproduce
/// is a `BLOCKED` finding, not a claimed pass).
pub fn run_risc0_verify(
    seal_bytes: &[u8],
    journal_bytes: &[u8],
    image_id: Option<&str>,
) -> Result<(), String> {
    let image_id = image_id.ok_or("imageId missing; cannot bind the claim")?;
    let image_hex = image_id
        .strip_prefix("sha256:")
        .ok_or("imageId not sha256:hex")?;
    let image_bytes = hex::decode(image_hex).map_err(|e| format!("imageId hex decode: {e}"))?;
    let image_bytes: [u8; 32] = image_bytes
        .try_into()
        .map_err(|_| "imageId is not 32 bytes".to_string())?;
    let image_digest = Digest::from(image_bytes);

    let ctx = VerifierContext::default();
    let params = ctx
        .groth16_verifier_parameters
        .as_ref()
        .ok_or("groth16 verifier parameters missing from context")?;
    let params_digest = params.digest::<ShaImpl>();

    // ReceiptClaim::ok takes the journal BYTES (MaybePruned<Vec<u8>>); the
    // claim's digest is computed over them, so we pass the hypothesis bytes.
    let claim = ReceiptClaim::ok(image_digest, MaybePruned::Value(journal_bytes.to_vec()));

    let inner = InnerReceipt::Groth16(Groth16Receipt::new(
        seal_bytes.to_vec(),
        MaybePruned::from(claim),
        params_digest,
    ));
    let receipt = Receipt::new(inner, journal_bytes.to_vec());

    receipt
        .verify_with_context(&ctx, image_digest)
        .map_err(|e| format!("risc0 verification error: {e}"))
}

/// Digest of a byte string, using risc0's sha256 (`Journal::digest`).
fn digest_of(bytes: &[u8]) -> Digest {
    Digestible::digest::<ShaImpl>(&Journal::new(bytes.to_vec()))
}

/// The risc0 journal digest of `bytes`, as a `sha256:<hex>` string.
pub fn journal_digest_hex(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(digest_of(bytes).as_bytes()))
}

/// The groth16 verifier parameters the crate embeds (control root, BN254 control
/// id, and the Groth16 verifying key), for the snarkjs independent-twin plan.
pub fn dumped_verifier_params() -> serde_json::Value {
    let ctx = VerifierContext::default();
    match ctx.groth16_verifier_parameters {
        Some(p) => serde_json::to_value(&p).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    }
}

/// Derive the claim digest and the five Groth16 public inputs for a receipt,
/// exactly as the risc0 on-chain verifier derives them, so a snarkjs twin has the
/// same input vector. Returns `{ claim_digest, journal_digest, control_root,
/// bn254_control_id, public_inputs:[crh_hi, crh_lo, clm_hi, clm_lo, bn254_cid] }`.
pub fn public_inputs_for(receipt_json: &str) -> Result<serde_json::Value, String> {
    let receipt: Value = serde_json::from_str(receipt_json).map_err(|e| e.to_string())?;
    let image_id = receipt
        .get("imageId")
        .and_then(|v| v.as_str())
        .ok_or("no imageId")?;
    let image_bytes = hex::decode(image_id.strip_prefix("sha256:").ok_or("not sha256:hex")?)
        .map_err(|e| e.to_string())?;
    let image_bytes: [u8; 32] = image_bytes
        .try_into()
        .map_err(|_| "imageId not 32 bytes".to_string())?;
    let image_digest = Digest::from(image_bytes);

    let journal = receipt.get("journal").ok_or("no journal")?;
    let jcs_bytes = canonical_jcs(journal);
    let journal_digest = digest_of(&jcs_bytes);

    let claim = ReceiptClaim::ok(image_digest, MaybePruned::Value(jcs_bytes));
    let claim_digest = claim.digest::<ShaImpl>();

    let ctx = VerifierContext::default();
    let params = ctx.groth16_verifier_parameters.ok_or("no groth16 params")?;
    let (crh, clm) = (
        split_digest(params.control_root),
        split_digest(claim_digest),
    );
    let mut public_inputs = vec![
        "0x".to_string() + &crh.0,
        "0x".to_string() + &crh.1,
        "0x".to_string() + &clm.0,
        "0x".to_string() + &clm.1,
    ];
    public_inputs.push(format!(
        "0x{}",
        hex::encode(params.bn254_control_id.as_bytes())
    ));
    Ok(serde_json::json!({
        "claim_digest": format!("0x{}", hex::encode(claim_digest.as_bytes())),
        "journal_digest": format!("0x{}", hex::encode(journal_digest.as_bytes())),
        "control_root": format!("0x{}", hex::encode(params.control_root.as_bytes())),
        "bn254_control_id": format!("0x{}", hex::encode(params.bn254_control_id.as_bytes())),
        "public_inputs": public_inputs,
    }))
}

/// Split a 32-byte digest into two 16-byte halves with the byte order the risc0
/// on-chain verifier uses (reverses the whole digest, then takes the two halves).
fn split_digest(digest: Digest) -> (String, String) {
    let bytes = digest.as_bytes();
    let mut rev = [0u8; 32];
    for i in 0..32 {
        rev[31 - i] = bytes[i];
    }
    (hex::encode(&rev[0..16]), hex::encode(&rev[16..32]))
}

/// The journal-bytes candidate **byte strings**, as `(label, bytes)` pairs.
///
/// This is the single source of truth for the candidate set; the per-receipt
/// hypothesis binary iterates it to attempt verification, and `journal_candidates`
/// derives the display digests from the same bytes. Keeping one list in the
/// crate prevents the two copies drifting.
pub fn journal_candidate_bytes(journal: &Value) -> Vec<(String, Vec<u8>)> {
    vec![
        ("RFC8785/JCS canonical".to_string(), canonical_jcs(journal)),
        (
            "compact JSON (no whitespace)".to_string(),
            serde_json::to_vec(journal).expect("Value serializes to bytes"),
        ),
        (
            "pretty JSON (2-space indent)".to_string(),
            serde_json::to_vec_pretty(journal).expect("Value serializes to bytes"),
        ),
    ]
}

/// Enumerate the journal-bytes candidates for the hypothesis test, with the
/// label used in the per-receipt table. The primary hypothesis is RFC 8785/JCS.
pub fn journal_candidates(journal: &Value) -> Vec<JournalCandidate> {
    let mut out = Vec::new();
    for (label, bytes) in journal_candidate_bytes(journal) {
        let digest = digest_of(&bytes);
        out.push(JournalCandidate {
            label,
            bytes_hex_prefix: hex::encode(&bytes).chars().take(24).collect(),
            digest: format!("sha256:{}", hex::encode(digest.as_bytes())),
        });
    }
    out
}

/// Best-effort RFC 8785 (JCS) canonicalization: recursive, sorted keys, no
/// whitespace, strings via JSON.stringify, numbers via JSON.stringify.
///
/// This is a faithful approximation for the finite numbers the journal
/// actually carries, but it is **not** byte-identical to RFC 8785 in two
/// corners (both fail *closed*: a journal hitting either gets NOT VALID rather
/// than a false pass):
///
/// - **Number serialization.** RFC 8785 §3.2.2.3 mandates ECMAScript
///   `Number::toString`, which renders `1`, `0`, `0`, `43210`. `serde_json`'s
///   float formatter emits `1.0`, `0.0`, `-0.0`, `43210.0` for a value that
///   parses to an `f64` (any literal containing `.` or `e`/`E`, and literal
///   `-0`). The scientific-notation exponent thresholds also differ.
/// - **Key sort order.** This implementation sorts object keys by UTF-8 byte
///   order (`BTreeMap`); RFC 8785 §3.2.3 requires UTF-16 code-unit order.
///   The two invert only for keys mixing U+E000–U+FFFF with U+10000+, which
///   the estate journals do not contain.
///
/// # Errors
/// None for JSON values; this is a lossless, deterministic transform.
pub fn canonical_jcs(value: &Value) -> Vec<u8> {
    let mut s = String::new();
    write_jcs(&mut s, value);
    s.into_bytes()
}

fn write_jcs(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => push_json_string(out, s),
        Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_jcs(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            // BTreeMap iteration is already sorted by key.
            let sorted: BTreeMap<&str, &Value> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
            out.push('{');
            for (i, (k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_string(out, k);
                out.push(':');
                write_jcs(out, v);
            }
            out.push('}');
        }
    }
}

fn push_json_string(out: &mut String, s: &str) {
    // JSON.stringify string escaping (subset sufficient for JCS).
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jcs_sorts_keys_and_strips_whitespace() {
        let v: Value =
            serde_json::from_str(r#"{ "b": 2, "a": [1, {"d": -1.5, "c": "x"} ] }"#).unwrap();
        let jcs = String::from_utf8(canonical_jcs(&v)).unwrap();
        assert_eq!(jcs, r#"{"a":[1,{"c":"x","d":-1.5}],"b":2}"#);
    }

    #[test]
    fn jcs_is_deterministic() {
        let v: Value = serde_json::from_str(r#"{"y":0,"x":"z"}"#).unwrap();
        assert_eq!(canonical_jcs(&v), canonical_jcs(&v));
    }

    #[test]
    fn candidates_cover_three_hypotheses() {
        let v: Value = serde_json::from_str(r#"{"k":"v"}"#).unwrap();
        let cands = journal_candidates(&v);
        assert_eq!(cands.len(), 3);
        assert!(cands[0].label.contains("JCS"));
    }

    #[test]
    fn verify_refuses_missing_seal() {
        let r = verify(
            r#"{"journal":{},"imageId":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#,
            None,
        );
        assert!(!r.valid);
        assert!(r
            .checks
            .iter()
            .any(|c| c.name == "seal decodes as base64" && c.passed == Some(false)));
    }
}

/// wasm32 bindings: expose `verify()` to the browser mirroring the crate
/// result shape exactly (named `Check`s preserved) as a JS object.
#[cfg(target_arch = "wasm32")]
pub mod bindings {
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen::JsValue;

    /// `verify(receipt_json, expected_image_id?)` -> a JS object matching
    /// `VerifyResult` ({valid, image_id, journal_digest, journal, checks:[...]}).
    ///
    /// `journal` is a `serde_json::Value::Object`; without
    /// `serialize_maps_as_objects(true)` the default `serde_wasm_bindgen`
    /// serializer turns it into a JS `Map`, which `JSON.stringify` renders as
    /// `{}` — the page's `#raw` panel would silently lose the journal.
    #[wasm_bindgen]
    pub fn verify(receipt_json: &str, expected_image_id: Option<String>) -> JsValue {
        use serde::Serialize;
        let result = crate::verify(receipt_json, expected_image_id.as_deref());
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        match result.serialize(&serializer) {
            Ok(value) => value,
            Err(_) => JsValue::NULL,
        }
    }
}
