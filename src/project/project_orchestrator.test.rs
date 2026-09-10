#![allow(non_snake_case)]
// Emphatic capitals in a test name are this codebase convention: a name that says WHAT IS
// BEING PROVEN reads better than one that obeys a lint, and a warning nobody clears becomes a
// warning nobody reads - which is how a real one gets missed.

//! Tests for the project workflow.
//!
//! THE ORDER IS WHAT IS BEING TESTED, not the individual steps — those have their own tests. What only
//! shows up here is that validation runs BEFORE the build and location runs AFTER it, because getting
//! either backwards is invisible in a unit test of the pieces and expensive in practice.

use super::*;

fn write(dir: &std::path::Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A project with no `build` declaration, so no toolchain runs and the test is fast and hermetic.
fn project(dir: &std::path::Path, artifacts: &str) -> () {
    write(
        dir,
        crate::project::types::MANIFEST_FILE,
        &format!(
            r#"{{ "fqid": "syw.probe.echo", "version": "1.0.0", "coreCompatibility": "^0.1", "artifacts": {artifacts} }}"#
        ),
    );
}

#[test]
fn a_malformed_manifest_is_refused_and_NOTHING_is_built() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        crate::project::types::MANIFEST_FILE,
        // A build declaration that points at a crate manifest which does not exist. If validation did
        // NOT run first, the build would be attempted and the error would name a missing Cargo.toml
        // rather than the malformed fqid that is actually wrong.
        r#"{ "fqid": "no-dots", "version": "1.0.0", "build": { "manifestPath": "nope/Cargo.toml" }, "artifacts": [] }"#,
    );

    let e = plan_bundle(dir.path(), false).unwrap_err().to_string();
    assert!(e.contains("fqid"), "the refusal must name the manifest problem, not a build failure: {e}");
    assert!(
        !e.to_lowercase().contains("cargo") && !e.to_lowercase().contains("toolchain"),
        "VALIDATION RUNS BEFORE THE BUILD: a release build costs minutes and a malformed fqid costs \
         microseconds to detect. got: {e}"
    );
}

#[test]
fn a_declared_artifact_missing_after_the_build_is_its_own_distinct_failure() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), r#"[{ "path": "out/libecho.so", "kind": "Dll", "entryPoint": "wf_init" }]"#);

    let e = plan_bundle(dir.path(), true).unwrap_err().to_string();
    // "the build succeeded and produced nothing at the declared path" must be SAYABLE. In the other
    // order — locating before building — a stale artifact from a forgotten build silently takes its
    // place and nothing can report that it happened.
    assert!(e.contains("out/libecho.so"), "got {e}");
    assert!(e.contains("does not exist"), "got {e}");
}

#[test]
fn a_plan_carries_paths_and_declarations_but_no_bytes_and_no_hashes() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), r#"[{ "path": "out/libecho.so", "kind": "Dll", "entryPoint": "wf_init" }]"#);
    write(dir.path(), "out/libecho.so", "not really a library");

    let (plan, report) = plan_bundle(dir.path(), true).unwrap();
    assert_eq!(plan.fqid, "syw.probe.echo");
    assert_eq!(plan.artifacts.len(), 1);
    assert_eq!(plan.artifacts[0].name, "libecho.so");
    // A PLAN, NOT A BUNDLE. Hashes are facts about the bytes actually written, and the writer that
    // writes them is the honest place to measure them.
    assert!(plan.manifest_body.get("artifacts").is_none(), "the artifacts array belongs to the writer");
    assert_eq!(plan.manifest_body["hosting_mode"], "DIRECT");
    // Whether a build ran is reported even when it did not.
    assert!(!report.ran, "--no-build must be visible in the report, not merely in its absence");
}

#[test]
fn the_namespace_segment_uuid_is_on_the_plan_so_the_writer_need_not_derive_it() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "[]");
    let (plan, _) = plan_bundle(dir.path(), true).unwrap();
    // Three places must agree: the ingested entity, the install-call uuid and the namespace segment
    // file name. Carrying one value rather than recomputing it downstream is what makes them agree by
    // construction.
    assert_eq!(plan.package_uuid, crate::project::manifest_compiler::package_uuid_for("syw.probe.echo"));
}

#[test]
fn validate_stops_before_the_build_even_when_a_build_is_declared() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        crate::project::types::MANIFEST_FILE,
        r#"{ "fqid": "syw.probe.echo", "version": "1.0.0", "build": { "manifestPath": "nope/Cargo.toml" }, "artifacts": [] }"#,
    );
    // IT EXISTS SO THE CHEAP CHECK IS AVAILABLE ALONE. A tool whose only way to check a manifest is to
    // build the package teaches people not to check — and a `validate` that built would be that tool.
    assert_eq!(validate_project(dir.path()).unwrap(), vec![]);
}

#[test]
fn a_project_with_no_build_declaration_still_packs() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "[]");
    // Not an error: a project whose artifacts are produced by something else entirely still packs, and
    // refusing it would make this tool the only way to build a Waffler package.
    let (plan, report) = plan_bundle(dir.path(), false).unwrap();
    assert_eq!(plan.manifest_body["hosting_mode"], "HOSTED");
    assert!(!report.ran);
}

#[test]
fn scaffolding_then_validating_the_result_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("hello");
    scaffold_project(&target, "syw.example.hello", "0.1.0", "A greeting.", "../..", false).unwrap();
    // THE WHOLE LOOP THROUGH THE FILESYSTEM: rendered, written, read back, and checked by the same code
    // a developer's own `waffler validate` runs. A template test alone would pass while the reader and
    // the writer disagreed about a field name.
    assert_eq!(validate_project(&target).unwrap(), vec![]);
}

#[test]
fn scaffolding_refuses_a_non_empty_directory_unless_told_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "keep.txt", "mine");
    assert!(scaffold_project(dir.path(), "syw.example.hello", "0.1.0", "x", "../..", false).is_err());
    assert_eq!(std::fs::read_to_string(dir.path().join("keep.txt")).unwrap(), "mine");
    scaffold_project(dir.path(), "syw.example.hello", "0.1.0", "x", "../..", true).unwrap();
    assert!(dir.path().join(crate::project::types::MANIFEST_FILE).exists());
}
