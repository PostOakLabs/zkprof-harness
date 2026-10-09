// SPDX-License-Identifier: Apache-2.0
//
// DECIDER TEST (art-04 JCS-vs-insertion-order decider): decide the journal-bytes contract using
// art-04, the ONE fixture in the estate compute-proof dir whose journal has
// UNSORTED (nested) keys. The three earlier fixtures were key-sorted, so their
// JCS and compact-insertion-order byte strings were identical; art-04 separates
// them. This test runs the full risc0 3.0.5 Groth16 verifier against BOTH the
// JCS-canonical bytes and the stored-insertion-order bytes and asserts that
// EXACTLY ONE lets verification pass.
//
// serde_json runs with the "preserve_order" feature so `to_vec(&journal)`
// yields the journal's stored insertion-order bytes (not a re-sorted form); the
// crate's `canonical_jcs` sorts keys explicitly, so it is the JCS form.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::Value;

fn load_art04() -> (Vec<u8>, String, Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/compute-proof/art-04.receipt.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let receipt: Value = serde_json::from_str(&text).unwrap();
    let seal_b64 = receipt.get("seal").unwrap().as_str().unwrap();
    let seal = B64.decode(seal_b64).unwrap();
    let image_id = receipt
        .get("imageId")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let journal = receipt.get("journal").unwrap().clone();
    (seal, image_id, journal)
}

#[test]
fn art04_decides_jcs_vs_insertion_order() {
    let (seal, image_id, journal) = load_art04();

    let jcs_bytes = zkprof_verify::canonical_jcs(&journal);
    let insertion_bytes = serde_json::to_vec(&journal).unwrap();

    let jcs_digest = zkprof_verify::journal_digest_hex(&jcs_bytes);
    let insertion_digest = zkprof_verify::journal_digest_hex(&insertion_bytes);

    let jcs_verify = zkprof_verify::run_risc0_verify(&seal, &jcs_bytes, Some(&image_id));
    let insertion_verify =
        zkprof_verify::run_risc0_verify(&seal, &insertion_bytes, Some(&image_id));

    println!("imageId:        {image_id}");
    println!("JCS digest:     {jcs_digest}");
    println!("JCS verify:     {}", out(&jcs_verify));
    println!("Insertion dgst: {insertion_digest}");
    println!("Insertion ver:  {}", out(&insertion_verify));

    let jcs_ok = jcs_verify.is_ok();
    let insertion_ok = insertion_verify.is_ok();

    // Exactly one derivation must pass the full Groth16 verification.
    assert!(
        jcs_ok ^ insertion_ok,
        "expected EXACTLY ONE of JCS / insertion-order to verify, but got \
         JCS={jcs_ok} insertion={insertion_ok}"
    );

    if jcs_ok {
        println!("DECISION: JCS (RFC 8785 canonical) wins for art-04.");
    } else {
        println!("DECISION: stored-insertion-order wins for art-04.");
    }
}

fn out(r: &Result<(), String>) -> String {
    match r {
        Ok(()) => "VERIFIED (Ok)".to_string(),
        Err(e) => format!("FAILED: {e}"),
    }
}
