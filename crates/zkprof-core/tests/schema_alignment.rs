// SPDX-License-Identifier: Apache-2.0

//! Structural alignment between the Rust model and the vendored schema.
//!
//! These tests read `zkprof-report.schema.json` and compare it against what the
//! [`zkprof_core::model`] structs actually serialise. They exist because the
//! schema is owned by a different repository: without a mechanical check, a
//! rename upstream would surface as a validator rejection at publication time
//! rather than as a build failure here.
//!
//! They deliberately do **not** validate documents against the schema — there
//! is no JSON Schema engine in this crate's dependency set, and adding one to
//! duplicate `edi-schema`'s Python validator is not worth the build cost. What
//! is checked here is the part a JSON Schema engine cannot catch: whether the
//! Rust types and the schema are describing the same document at all.

use std::collections::BTreeSet;

use serde_json::Value;
use zkprof_core::model::{
    GpuTimingSource, Memory, Phase, Primitive, PrimitiveRow, Report, System, Target, TargetKind,
    Threads,
};
use zkprof_core::{Primitive as RePrimitive, SCHEMA_JSON};

/// Keys of a JSON object.
fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .cloned()
        .collect()
}

/// Keys of `properties` at `path` inside the schema.
fn schema_keys(schema: &Value, path: &[&str]) -> BTreeSet<String> {
    let mut node = schema.clone();
    for segment in path {
        node = node
            .get(segment)
            .unwrap_or_else(|| panic!("schema has no {segment:?} at {path:?}"))
            .clone();
    }
    keys(&node)
}

fn schema() -> Value {
    serde_json::from_str(SCHEMA_JSON).expect("the vendored schema must parse")
}

/// The `required` list at `path`, as a set.
fn required(schema: &Value, path: &[&str]) -> BTreeSet<String> {
    let mut node = schema.clone();
    for segment in path {
        node = node.get(segment).expect("missing schema path").clone();
    }
    node.get("required")
        .map(|list| {
            list.as_array()
                .expect("`required` must be an array")
                .iter()
                .map(|item| {
                    item.as_str()
                        .expect("`required` items must be strings")
                        .to_owned()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The enum values at `path`, in schema order.
fn schema_enum(schema: &Value, path: &[&str]) -> Vec<String> {
    let mut node = schema.clone();
    for segment in path {
        node = node.get(segment).expect("missing schema path").clone();
    }
    node.get("enum")
        .expect("expected an enum at this path")
        .as_array()
        .expect("`enum` must be an array")
        .iter()
        .map(|item| {
            item.as_str()
                .expect("`enum` items must be strings")
                .to_owned()
        })
        .collect()
}

/// A report with every optional field populated, so the serialised key set is
/// the full key set.
fn fully_populated_report() -> Report {
    Report {
        schema: Some(zkprof_core::SCHEMA_ID.to_owned()),
        report_version: zkprof_core::REPORT_VERSION.to_owned(),
        target: Target {
            kind: TargetKind::WasmSimd,
            browser: Some("Chrome 141".to_owned()),
            os: "linux".to_owned(),
            cpu: "x86_64".to_owned(),
            gpu: Some("Apple M2 Pro".to_owned()),
        },
        system: System::new("groth16", "bn254").with_constraint_count(16_384),
        workload_id: "alignment-fixture".to_owned(),
        total_ms: 12.5,
        phases: vec![Phase {
            name: "setup".to_owned(),
            ms: 2.5,
            share: 0.2,
        }],
        primitives: vec![
            PrimitiveRow {
                name: Primitive::Msm,
                ms: 7.5,
                share: 0.75,
            },
            PrimitiveRow {
                name: Primitive::Other,
                ms: 2.5,
                share: 0.25,
            },
        ],
        memory: Memory::peak_heap_bytes(2_097_152),
        threads: Threads {
            count: 4,
            shared_array_buffer: true,
        },
        cross_origin_isolated: true,
        timer_resolution_us: 5.0,
        iterations: 32,
        run_manifest_ref: Some("runs/manifest-0001.json".to_owned()),
        gpu_timing_source: Some(GpuTimingSource::TimestampQuery),
        backend_id: Some("arkworks-groth16".to_owned()),
        version: Some("0.1.0".to_owned()),
        commitment_scheme: Some("groth16".to_owned()),
    }
}

#[test]
fn the_report_serialises_exactly_the_keys_the_schema_declares() {
    let schema = schema();
    let report_keys = keys(&serde_json::to_value(fully_populated_report()).unwrap());
    let schema_property_keys = schema_keys(&schema, &["properties"]);

    let extra: Vec<&String> = report_keys.difference(&schema_property_keys).collect();
    assert!(
        extra.is_empty(),
        "the Rust model emits keys the schema does not declare: {extra:?}"
    );
}

#[test]
fn the_report_serialises_every_required_key() {
    let schema = schema();
    let report_keys = keys(&serde_json::to_value(fully_populated_report()).unwrap());
    let required_keys = required(&schema, &[]);

    let missing: Vec<&String> = required_keys.difference(&report_keys).collect();
    assert!(
        missing.is_empty(),
        "the Rust model omits required keys: {missing:?}"
    );
}

#[test]
fn the_only_optional_keys_are_the_three_the_schema_makes_optional() {
    let schema = schema();
    let report_keys = keys(&serde_json::to_value(fully_populated_report()).unwrap());
    let required_keys = required(&schema, &[]);

    let mut optional: Vec<String> = report_keys.difference(&required_keys).cloned().collect();
    optional.sort();

    assert_eq!(
        optional,
        vec![
            "$schema",
            "backend_id",
            "commitment_scheme",
            "gpu_timing_source",
            "run_manifest_ref",
            "version"
        ]
    );
}

#[test]
fn the_gpu_timing_source_enum_matches_the_schema_spelling_and_order() {
    let schema = schema();
    let expected = schema_enum(&schema, &["properties", "gpu_timing_source"]);

    let actual: Vec<String> = GpuTimingSource::ALL
        .iter()
        .map(|source| source.as_str().to_owned())
        .collect();

    assert_eq!(actual, expected);
}

#[test]
fn a_report_without_a_gpu_timing_source_omits_the_key() {
    // Absent means "this report attributes no time to a GPU", which is a
    // different statement from any value the field can carry. A `None` that
    // serialised as null would collapse the two.
    let mut report = fully_populated_report();
    report.gpu_timing_source = None;

    let value = serde_json::to_value(&report).unwrap();
    assert!(!value.as_object().unwrap().contains_key("gpu_timing_source"));
}

#[test]
fn the_primitive_enum_matches_the_schema_spelling_and_order() {
    let schema = schema();
    let expected = schema_enum(
        &schema,
        &["properties", "primitives", "items", "properties", "name"],
    );

    let actual: Vec<String> = RePrimitive::ALL
        .iter()
        .map(|primitive| primitive.as_str().to_owned())
        .collect();

    assert_eq!(actual, expected);
}

#[test]
fn the_target_kind_enum_matches_the_schema_spelling_and_order() {
    let schema = schema();
    let expected = schema_enum(&schema, &["properties", "target", "properties", "kind"]);

    let actual: Vec<String> = TargetKind::ALL
        .iter()
        .map(|kind| kind.as_str().to_owned())
        .collect();

    assert_eq!(actual, expected);
}

#[test]
fn every_sub_object_emits_exactly_the_keys_its_schema_block_declares() {
    let schema = schema();
    let report = fully_populated_report();

    let cases: Vec<(&str, Value, Vec<&str>)> = vec![
        (
            "target",
            serde_json::to_value(&report.target).unwrap(),
            vec!["properties", "target", "properties"],
        ),
        (
            "system",
            serde_json::to_value(&report.system).unwrap(),
            vec!["properties", "system", "properties"],
        ),
        (
            "memory",
            serde_json::to_value(report.memory).unwrap(),
            vec!["properties", "memory", "properties"],
        ),
        (
            "threads",
            serde_json::to_value(report.threads).unwrap(),
            vec!["properties", "threads", "properties"],
        ),
        (
            "primitives row",
            serde_json::to_value(&report.primitives[0]).unwrap(),
            vec!["properties", "primitives", "items", "properties"],
        ),
        (
            "phase row",
            serde_json::to_value(&report.phases[0]).unwrap(),
            vec!["properties", "phases", "items", "properties"],
        ),
    ];

    for (what, value, path) in cases {
        assert_eq!(
            keys(&value),
            schema_keys(&schema, &path),
            "{what} emits a different key set than the schema declares"
        );
    }
}

#[test]
fn every_sub_object_emits_every_required_key() {
    let schema = schema();
    let report = fully_populated_report();

    let cases: Vec<(&str, Value, Vec<&str>)> = vec![
        (
            "target",
            serde_json::to_value(&report.target).unwrap(),
            vec!["properties", "target"],
        ),
        (
            "system",
            serde_json::to_value(&report.system).unwrap(),
            vec!["properties", "system"],
        ),
        (
            "threads",
            serde_json::to_value(report.threads).unwrap(),
            vec!["properties", "threads"],
        ),
    ];

    for (what, value, path) in cases {
        let required_keys = required(&schema, &path);
        let value_keys = keys(&value);
        let missing: Vec<&String> = required_keys.difference(&value_keys).collect();
        assert!(
            missing.is_empty(),
            "{what} omits required keys: {missing:?}"
        );
    }
}

#[test]
fn the_schema_closes_every_object_it_declares() {
    // `additionalProperties: false` is what turns a typo into an error rather
    // than an ignored field. If it is ever relaxed upstream, this fails and
    // somebody has to decide that on purpose.
    let schema = schema();
    let json = serde_json::to_string(&schema).unwrap();
    let false_count = json.matches("\"additionalProperties\":false").count()
        + json.matches("\"additionalProperties\": false").count();

    // Top level, target, system, phases.items, primitives.items, memory,
    // threads: seven objects.
    assert_eq!(
        false_count, 7,
        "expected seven closed objects in the schema"
    );
}

#[test]
fn the_report_version_in_the_crate_is_a_valid_schema_version_string() {
    let report = fully_populated_report();
    assert!(serde_json::to_value(&report).unwrap()["report_version"]
        .as_str()
        .unwrap()
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())));
}
