//! Tests for the scaffold.
//!
//! THE POINT OF THESE IS THE CROSS-CHECK, NOT THE TEMPLATE. A scaffold is a claim about the current
//! format, and the only way that claim stays true is by handing what it renders to the code that packs
//! and refusing it there when it is wrong. Asserting the template's text would pass forever while the
//! format moved underneath it — which is exactly how the previous scaffold shipped the legacy manifest
//! model for four months.

use super::*;
use crate::project::{manifest_compiler, types::AuthoredPackage};

fn manifest_of(files: &[RenderedFile]) -> AuthoredPackage {
    let raw = &files
        .iter()
        .find(|f| f.relative_path == crate::project::types::MANIFEST_FILE)
        .expect("a scaffold must render a manifest")
        .contents;
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("the rendered manifest does not parse: {e}\n{raw}"))
}

#[test]
fn what_the_scaffold_renders_passes_every_check_that_pack_applies() {
    let files = render_project("syw.example.hello", "0.1.0", "A greeting.", "../..");
    let manifest = manifest_of(&files);
    // THE CROSS-CHECK. If a rule is added to the compiler that the template violates, this fails here
    // rather than on a developer's first publish, with a message from a registry that cannot explain
    // which of the twenty things they changed was already wrong.
    assert_eq!(
        manifest_compiler::validate_authored(&manifest),
        vec![],
        "the generated manifest must satisfy the same rules `waffler pack` enforces"
    );
}

#[test]
fn the_declared_artifact_path_matches_the_crate_the_generated_cargo_manifest_builds() {
    let files = render_project("syw.example.hello", "0.1.0", "A greeting.", "../..");
    let manifest = manifest_of(&files);
    let cargo = &files.iter().find(|f| f.relative_path == "Cargo.toml").unwrap().contents;

    // ONE DERIVATION, ASSERTED ACROSS TWO FILES. The crate name and the declared artifact path are the
    // same fact written twice, and the failure when they diverge is "the build succeeded and produced
    // nothing at the declared path" — reported at pack time, naming a path the developer never typed.
    assert!(cargo.contains("name = \"syw_example_hello\""), "got:\n{cargo}");
    let declared = &manifest.artifacts[0].path;
    assert!(declared.contains("syw_example_hello"), "the declared path must name the crate that is built: {declared}");
    assert!(declared.starts_with("target/release/"), "release, because a debug module exceeds core's inline custody cap: {declared}");
}

#[test]
fn the_generated_crate_carries_its_own_empty_workspace_table() {
    let files = render_project("syw.example.hello", "0.1.0", "A greeting.", "../..");
    let cargo = &files.iter().find(|f| f.relative_path == "Cargo.toml").unwrap().contents;
    // WITHOUT IT the crate refuses to build at all inside a checkout that lists its path in neither
    // `members` nor `exclude` — "current package believes it's in a workspace when it's not". A scaffold
    // that omitted it would generate a project that cannot be built from the directory it was created in.
    assert!(cargo.contains("[workspace]"), "got:\n{cargo}");
}

#[test]
fn the_generated_crate_is_both_cdylib_and_rlib() {
    let files = render_project("syw.example.hello", "0.1.0", "A greeting.", "../..");
    let cargo = &files.iter().find(|f| f.relative_path == "Cargo.toml").unwrap().contents;
    // cdylib is what a node loads; rlib is what `cargo test` links. A scaffold that emitted only the
    // first would generate a package whose own tests cannot run, which teaches a developer that a
    // Waffler package is untestable.
    assert!(cargo.contains("cdylib"), "got:\n{cargo}");
    assert!(cargo.contains("rlib"), "got:\n{cargo}");
}

#[test]
fn the_generated_source_serves_a_capability_that_ANSWERS() {
    let files = render_project("syw.example.hello", "0.1.0", "A greeting.", "../..");
    let lib = &files.iter().find(|f| f.relative_path == "src/lib.rs").unwrap().contents;
    // "Installed" and "working" must be different observations. A generated package with no capability
    // leaves only a row in the package list as evidence it exists — which a records-only install would
    // produce just as well — so a developer would learn to stop one step too early.
    assert!(lib.contains("CAP_ECHO"), "the generated package must serve something");
    assert!(lib.contains("waffler_direct_package!"), "and register itself with the SDK");
    // The reply carries the package's own identity, so a caller can tell WHICH package answered.
    assert!(lib.contains("\"package\": FQID"), "the reply must identify the artifact that produced it");
    // MessagePack, because that is what the bus carries and what a caller decodes.
    assert!(lib.contains("to_vec_named"), "the reply must be encoded as the bus's own encoding");
}

#[test]
fn the_sdk_path_reaches_the_generated_dependencies() {
    let files = render_project("syw.example.hello", "0.1.0", "A greeting.", "/opt/waffler");
    let cargo = &files.iter().find(|f| f.relative_path == "Cargo.toml").unwrap().contents;
    // The SDK is not published, so a generated crate has to point somewhere real. A default that
    // pointed nowhere would produce a project that cannot build — the exact failure a scaffold exists
    // to prevent, moved from the manifest into the crate manifest.
    assert!(cargo.contains("/opt/waffler/sdk/rust"), "got:\n{cargo}");
    assert!(cargo.contains("/opt/waffler/shared"), "got:\n{cargo}");
}

#[test]
fn the_generated_manifest_pins_a_core_contract_range() {
    let files = render_project("syw.example.hello", "0.1.0", "A greeting.", "../..");
    let manifest = manifest_of(&files);
    // A bundle declaring none installs UNCHECKED behind a warning nobody reads, and a scaffold is the
    // one place where the right declaration costs the developer nothing.
    assert_eq!(manifest.core_compatibility.as_deref(), Some(SCAFFOLD_CORE_COMPATIBILITY));
}

#[test]
fn a_dotted_and_hyphenated_fqid_both_reduce_to_one_legal_crate_name() {
    let files = render_project("syw.my-package.thing", "0.1.0", "x", "../..");
    let cargo = &files.iter().find(|f| f.relative_path == "Cargo.toml").unwrap().contents;
    // A hyphen is legal in a cargo package name but NOT in the library file name cargo emits — it
    // becomes an underscore there — so a declared artifact path built from the unreduced name would
    // point at a file that never exists.
    assert!(cargo.contains("name = \"syw_my_package_thing\""), "got:\n{cargo}");
    let manifest = manifest_of(&files);
    assert!(manifest.artifacts[0].path.contains("syw_my_package_thing"));
    assert_eq!(manifest_compiler::validate_authored(&manifest), vec![]);
}
