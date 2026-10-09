// SPDX-License-Identifier: Apache-2.0
//
// Integration test: verify real estate fixtures and the mutation controls.
//
// A verifier that cannot fail is worse than one that says NOT-VERIFIED. These
// tests prove the crate verifies a real receipt AND that each targeted mutation
// (tampered seal / tampered journal / wrong imageId) fails with its distinct
// named check / verification error — the mutation-test discipline.
//
// The fixtures are read-only estate copies (see fixtures/compute-proof/PROVENANCE.md).

use std::path::Path;

use base64::Engine as _;
use serde_json::Value;
use zkprof_verify::verify;

fn fixture(name: &str) -> Value {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/compute-proof");
    let path = dir.join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn fixtures() -> [&'static str; 3] {
    [
        "art-01.receipt.json",
        "art-106.receipt.json",
        "rca-01.receipt.json",
    ]
}

#[test]
fn real_fixtures_verify() {
    // The headline hypothesis result: with JCS-canonical journal bytes these
    // verify end-to-end (no `expected_image_id`, so the imageId check is shape).
    for name in fixtures() {
        let receipt = fixture(name);
        let result = verify(&receipt.to_string(), None);
        assert!(
            result.valid,
            "{name} should verify; checks: {:?}",
            result.checks
        );
        // The named checks that must be PASS:
        for want in [
            "seal decodes as base64",
            "imageId is sha256:hex",
            "journal present",
            "receipt type/system/receiptFormat supported",
            "seal valid (Groth16 over BN254)",
        ] {
            let c = result.checks.iter().find(|c| c.name == want);
            assert!(c.is_some(), "{name}: missing check {want}");
            assert_eq!(c.unwrap().passed, Some(true), "{name}: {want} should pass");
        }
        assert!(result
            .journal_digest
            .as_deref()
            .unwrap_or("")
            .starts_with("sha256:"));
    }
}

#[test]
fn tampering_the_seal_must_fail() {
    // Flip one byte of the seal -> the Groth16 proof is no longer valid.
    for name in fixtures() {
        let mut receipt = fixture(name);
        let seal = receipt
            .get_mut("seal")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(seal)
            .unwrap();
        bytes[40] ^= 0xff; // flip a byte inside the proof
        let tampered = base64::engine::general_purpose::STANDARD.encode(bytes);
        receipt["seal"] = Value::String(tampered);

        let result = verify(&receipt.to_string(), None);
        assert!(!result.valid, "{name}: tampered seal must not verify");
        let seal_check = result
            .checks
            .iter()
            .find(|c| c.name == "seal valid (Groth16 over BN254)");
        assert_eq!(
            seal_check.map(|c| c.passed),
            Some(Some(false)),
            "{name}: seal-valid check must be false"
        );
    }
}

#[test]
fn tampering_the_journal_must_fail() {
    // Change a journal value -> JCS bytes differ -> claim digest differs -> invalid.
    for name in fixtures() {
        let mut receipt = fixture(name);
        // Mutate a scalar leaf if present, else add a marker.
        if let Some(out) = receipt.get_mut("journal").and_then(|j| j.get_mut("output")) {
            if out.is_object() {
                out["__tamper__"] = Value::Bool(true);
            } else {
                receipt["journal"]["__tamper__"] = Value::Bool(true);
            }
        }
        let result = verify(&receipt.to_string(), None);
        assert!(!result.valid, "{name}: tampered journal must not verify");
    }
}

#[test]
fn wrong_image_id_must_fail() {
    // Supply a different expected_image_id -> the imageId-expected check is false.
    for name in fixtures() {
        let receipt = fixture(name);
        let wrong = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        let result = verify(&receipt.to_string(), Some(wrong));
        assert!(!result.valid, "{name}: wrong expected imageId must fail");
        let check = result.checks.iter().find(|c| c.name == "imageId expected");
        assert_eq!(
            check.map(|c| c.passed),
            Some(Some(false)),
            "{name}: imageId-expected check must be false"
        );
    }
}

#[test]
fn jcs_is_key_sorted_and_compact() {
    let v: Value = serde_json::from_str(r#"{ "b": 1, "a": [ { "z": true }, "x" ] }"#).unwrap();
    let jcs = String::from_utf8(zkprof_verify::canonical_jcs(&v)).unwrap();
    assert_eq!(jcs, r#"{"a":[{"z":true},"x"],"b":1}"#);
}
