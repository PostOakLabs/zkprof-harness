// SPDX-License-Identifier: Apache-2.0

//! The vendored report schema and its provenance.
//!
//! `schema/zkprof-report.schema.json` is a vendored copy of the report schema
//! owned by `PostOakLabs/engineering-decision-schema`. The copy is currently
//! ahead of upstream (it adds `backend_id`, `commitment_scheme` and `version`;
//! see `tools/check_vendored_schema.py`). It lives in this repo so a `cargo test`
//! run can check the Rust structs against it without a network fetch or a
//! path-dependency on another repository.
//!
//! The copy is pinned by hash. `SCHEMA_SHA256` is not verified by this crate —
//! Rust has no sha256 in its standard library, and pulling in a hashing
//! dependency to guard one vendored file is worse than the guard. The check
//! lives in `tools/check_vendored_schema.py`, which CI runs.

/// The schema document, embedded at compile time.
pub const SCHEMA_JSON: &str = include_str!("../schema/zkprof-report.schema.json");

/// `$id` the schema declares for itself. Emitted as the `$schema` key of every
/// report so `edi-validate` can identify the document kind.
pub const SCHEMA_ID: &str =
    "https://postoaklabs.github.io/engineering-decision-schema/zkprof-report.schema.json";

/// sha256 of `schema/zkprof-report.schema.json`.
///
/// Pins the vendored copy: `tools/check_vendored_schema.py` (run by CI) fails if
/// the file changes without this constant changing with it.
pub const SCHEMA_SHA256: &str = "6ac3b291715a7efc1acd7fc0a50ad6fb3acc6a877d09fe3b8e099a39aab49bfe";

/// Version stamped into every report this crate emits.
pub const REPORT_VERSION: &str = "0.1.0";

#[cfg(test)]
mod tests {
    use super::{REPORT_VERSION, SCHEMA_ID, SCHEMA_JSON};

    #[test]
    fn the_vendored_schema_is_valid_json() {
        let schema: serde_json::Value =
            serde_json::from_str(SCHEMA_JSON).expect("the vendored schema must parse");
        assert_eq!(schema["$id"], SCHEMA_ID);
    }

    #[test]
    fn the_vendored_schema_uses_draft_2020_12() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA_JSON).unwrap();
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
    }

    #[test]
    fn the_report_version_is_a_semantic_version() {
        let parts: Vec<&str> = REPORT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{REPORT_VERSION} is not X.Y.Z");
        for part in parts {
            assert!(!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[test]
    fn the_declared_id_matches_what_the_schema_says_about_itself() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA_JSON).unwrap();
        assert_eq!(schema["$id"].as_str(), Some(SCHEMA_ID));
    }
}
