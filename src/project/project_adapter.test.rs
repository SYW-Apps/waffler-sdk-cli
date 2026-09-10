//! Tests for the project filesystem adapter.

use super::*;

fn write(dir: &std::path::Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

const SOUND_MANIFEST: &str = r#"{
  "fqid": "syw.probe.echo",
  "version": "1.0.0",
  "coreCompatibility": "^0.1",
  "build": { "manifestPath": "Cargo.toml" },
  "artifacts": [{ "path": "out/libecho.so", "kind": "Dll", "entryPoint": "wf_init" }]
}"#;

#[test]
fn a_manifest_round_trips_through_the_camelCase_wire_names() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), MANIFEST_FILE, SOUND_MANIFEST);

    let m = read_authored_manifest(dir.path()).unwrap();
    assert_eq!(m.fqid, "syw.probe.echo");
    // THE WIRE NAMES ARE THE ASSERTION. A developer writes `coreCompatibility`, `manifestPath` and
    // `entryPoint`; if the struct expected snake_case, every one of these would silently default and
    // the manifest would be read as declaring nothing — which parses.
    assert_eq!(m.core_compatibility.as_deref(), Some("^0.1"));
    assert_eq!(m.build.as_ref().unwrap().manifest_path, "Cargo.toml");
    assert_eq!(m.artifacts[0].entry_point.as_deref(), Some("wf_init"));
}

#[test]
fn a_legacy_package_json_is_named_in_the_refusal_rather_than_read() {
    let dir = tempfile::tempdir().unwrap();
    // The previous model. It would PARSE as the new one — every field it lacks is defaulted — and
    // produce a bundle wrong in every field.
    write(dir.path(), LEGACY_MANIFEST_FILE, r#"{"id":"echo","namespace":"syw.probe.echo","version":"1.0.0"}"#);

    let e = read_authored_manifest(dir.path()).unwrap_err().to_string();
    assert!(e.contains(MANIFEST_FILE), "the message must name what is missing: {e}");
    assert!(e.contains(LEGACY_MANIFEST_FILE), "and what was found instead: {e}");
    // The trap is the rename, so the message has to close it before the developer thinks of it.
    assert!(e.to_lowercase().contains("renam"), "it must say renaming is not the fix: {e}");
}

#[test]
fn a_missing_manifest_with_no_legacy_file_names_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let e = read_authored_manifest(dir.path()).unwrap_err().to_string();
    assert!(e.contains(MANIFEST_FILE));
    // NO MENTION OF THE LEGACY FILE HERE. Naming a file that is not present would send a developer
    // looking for something that does not exist — the two situations need different sentences, which is
    // why the check is on presence rather than on the manifest being absent.
    assert!(!e.contains(LEGACY_MANIFEST_FILE), "it must not mention a file that is not there: {e}");
}

#[test]
fn a_declared_path_is_resolved_EXACTLY_and_nothing_else_is_searched() {
    let dir = tempfile::tempdir().unwrap();
    // A plausible stale artifact in the place the PREVIOUS tool searched. It must not be found.
    write(dir.path(), "target/release/libecho.so", "stale");

    let declared = DeclaredArtifact {
        path: "out/libecho.so".into(),
        kind: "Dll".into(),
        entry_point: Some("wf_init".into()),
        name: None,
    };
    let e = locate_artifact(dir.path(), &declared).unwrap_err().to_string();
    assert!(e.contains("out/libecho.so"), "the message names the path AS DECLARED: {e}");
    // BOTH SPELLINGS. A relative path that looks right and a working directory that is not what the
    // developer thinks are the same mistake wearing different clothes.
    assert!(e.contains("resolved to"), "and the absolute path it resolved to: {e}");

    // Now the declared path exists, and the size is measured.
    write(dir.path(), "out/libecho.so", "fresh!");
    let located = locate_artifact(dir.path(), &declared).unwrap();
    assert_eq!(located.name, "libecho.so");
    assert_eq!(located.size_bytes, 6, "measured at location time so an oversized bundle is refused before an upload");
}

#[test]
fn a_directory_at_the_declared_path_is_refused_rather_than_packed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("out/libecho.so")).unwrap();
    let declared = DeclaredArtifact { path: "out/libecho.so".into(), kind: "Dll".into(), entry_point: None, name: None };
    let e = locate_artifact(dir.path(), &declared).unwrap_err().to_string();
    // `canonicalize` SUCCEEDS on a directory, so without the explicit file check this would pass
    // location and fail later inside the archive writer with a message about reading bytes.
    assert!(e.contains("not a file"), "got {e}");
}

#[test]
fn an_absent_namespace_directory_is_an_empty_result_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    // A package need not ship entities, and turning that into an error would make the common case
    // explain itself.
    assert!(read_namespace_tree(dir.path()).unwrap().is_empty());
}

#[test]
fn namespace_entries_are_forward_slashed_and_ordered() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "namespace/zeta/type.json", "{}");
    write(dir.path(), "namespace/alpha/.ns", "{}");
    write(dir.path(), "namespace/alpha/Entity/type.json", "{}");

    let files = read_namespace_tree(dir.path()).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
    // FORWARD SLASHES REGARDLESS OF PLATFORM: a zip entry named with a backslash is a zip entry whose
    // path component is a filename on every reader that matters, so a bundle packed on Windows would
    // carry entries no Linux node can find.
    assert!(names.iter().all(|n| !n.contains('\\')), "got {names:?}");
    // SORTED, so a bundle packed twice from one tree has its entries in one order — a directory walk's
    // order is filesystem-dependent, and bytes that vary for no reason anybody chose are bytes nobody
    // can compare.
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    assert_eq!(names.len(), 3);
}

#[test]
fn a_non_empty_target_is_refused_BEFORE_any_file_is_written() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "existing.txt", "keep me");

    let files = vec![
        RenderedFile { relative_path: "a.txt".into(), contents: "a".into() },
        RenderedFile { relative_path: "b.txt".into(), contents: "b".into() },
    ];
    assert!(write_project_files(dir.path(), &files, false).is_err());
    // THE REFUSAL COMES FIRST. A scaffold that checked per-file would have written `a.txt` already, and
    // a half-written project is worse than either outcome it was choosing between.
    assert!(!dir.path().join("a.txt").exists(), "nothing may be written before the refusal");
    assert_eq!(std::fs::read_to_string(dir.path().join("existing.txt")).unwrap(), "keep me");

    // With permission, both are written.
    write_project_files(dir.path(), &files, true).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("b.txt")).unwrap(), "b");
}

#[test]
fn an_empty_directory_is_written_into_without_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let files = vec![RenderedFile { relative_path: "src/lib.rs".into(), contents: "//! hi".into() }];
    // The ordinary scaffold case: parents are created, and an empty directory is not a collision.
    write_project_files(dir.path(), &files, false).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap(), "//! hi");
}
