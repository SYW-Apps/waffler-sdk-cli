//! Tests for the bundle writer.
//!
//! THE ASSERTIONS ARE ABOUT THE ARCHIVE'S CONTENTS, not about the function returning Ok. The whole
//! value of this component is that a registry and a node months later can read what it wrote, so what
//! is checked is entry names, entry contents, and the hashes measured over the bytes that went in.

use super::*;
use crate::project::types::{BundlePlan, LocatedArtifact, NamespaceFile};

fn plan_with(dir: &std::path::Path, artifacts: Vec<LocatedArtifact>, namespace_files: Vec<NamespaceFile>) -> BundlePlan {
    BundlePlan {
        fqid: "syw.probe.echo".into(),
        version: "1.0.0".into(),
        package_uuid: crate::project::manifest_compiler::package_uuid_for("syw.probe.echo"),
        manifest_body: serde_json::json!({
            "fqid": "syw.probe.echo", "version": "1.0.0", "hosting_mode": "DIRECT",
            "permission_groups": [], "dependencies": [], "capabilities": [],
            "ui_plugins": [], "middleware": [], "fast_lane_requests": [], "enabled": true
        }),
        artifacts,
        namespace_files,
        project_directory: dir.to_path_buf(),
    }
}

fn artifact(dir: &std::path::Path, name: &str, contents: &[u8]) -> LocatedArtifact {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    LocatedArtifact {
        name: name.to_string(),
        kind: "Dll".into(),
        entry_point: Some("wf_init".into()),
        absolute_path: path,
        size_bytes: contents.len() as u64,
    }
}

fn entries(path: &std::path::Path) -> Vec<String> {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().to_string()).collect()
}

fn read_entry(path: &std::path::Path, name: &str) -> Vec<u8> {
    use std::io::Read;
    let file = std::fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut out = Vec::new();
    archive.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

#[test]
fn the_layout_is_manifest_artifact_and_namespace_not_package_json_at_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan_with(dir.path(), vec![artifact(dir.path(), "libecho.so", b"ELF-ish")], vec![]);
    let out = dir.path().join("bundle.zip");
    bundle_writer_write(&plan, &out);

    let names = entries(&out);
    assert!(names.contains(&".manifest".to_string()), "got {names:?}");
    assert!(names.contains(&"artifact/libecho.so".to_string()), "got {names:?}");
    // `package.json` at the root is the layout the PREVIOUS tool wrote, and the registry's bundle
    // reader refuses it by explicit test.
    assert!(!names.iter().any(|n| n == "package.json"), "got {names:?}");
    // The identity segment, without which the staged reader finds no segment and ingests nothing — the
    // package installs as a record with no entity behind it, which looks like a successful install.
    assert!(
        names.iter().any(|n| n == &format!("namespace/{}.json", plan.package_uuid)),
        "got {names:?}"
    );
}

#[test]
fn artifact_hashes_are_MEASURED_over_the_bytes_written() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = b"the actual artifact contents";
    let plan = plan_with(dir.path(), vec![artifact(dir.path(), "libecho.so", bytes)], vec![]);
    let out = dir.path().join("bundle.zip");
    bundle_writer_write(&plan, &out);

    let manifest: serde_json::Value = serde_json::from_slice(&read_entry(&out, ".manifest")).unwrap();
    let expected = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(bytes))
    };
    // A HASH COMPUTED OVER THE BYTES BEING WRITTEN IS A FACT; one carried in from a manifest a developer
    // wrote is a claim, and the two disagreeing is exactly what a content hash exists to detect.
    assert_eq!(manifest["artifacts"][0]["hash"], expected);
    assert_eq!(manifest["artifacts"][0]["name"], "libecho.so");
    assert_eq!(manifest["artifacts"][0]["entry_point"], "wf_init");

    // AND THE EMBEDDED BYTES ARE THE ONES HASHED. Without this, a hash of the right file written beside
    // the wrong bytes would pass the assertion above.
    assert_eq!(read_entry(&out, "artifact/libecho.so"), bytes);
}

#[test]
fn entries_are_STORED_so_a_declared_size_is_the_real_size() {
    let dir = tempfile::tempdir().unwrap();
    // Highly compressible, so a deflated entry would be obviously smaller than its declared size.
    let bytes = vec![b'a'; 4096];
    let plan = plan_with(dir.path(), vec![artifact(dir.path(), "libecho.so", &bytes)], vec![]);
    let out = dir.path().join("bundle.zip");
    bundle_writer_write(&plan, &out);

    let file = std::fs::File::open(&out).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let entry = archive.by_name("artifact/libecho.so").unwrap();
    // The registry refuses a metadata entry that EXPANDS past its limit, and a compressed entry can
    // declare one size and expand to another — a decompression bomb that was accepted and published
    // before the reader learned to check both numbers. Storing removes the gap entirely.
    assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
    assert_eq!(entry.size(), entry.compressed_size(), "stored means the two numbers cannot differ");
}

#[test]
fn the_projects_own_namespace_files_are_copied_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let entity = dir.path().join("Entity.json");
    std::fs::write(&entity, br#"{"uuid":"abc","entity_type":"type"}"#).unwrap();
    let plan = plan_with(
        dir.path(),
        vec![],
        vec![NamespaceFile { relative: "syw/probe/Entity.json".into(), absolute: entity }],
    );
    let out = dir.path().join("bundle.zip");
    bundle_writer_write(&plan, &out);

    // READ, NOT PARSED AND RE-EMITTED. The segment format belongs to VFS's staged reader, and a second
    // implementation of a format this tool does not own is the one that drifts.
    assert_eq!(read_entry(&out, "namespace/syw/probe/Entity.json"), br#"{"uuid":"abc","entity_type":"type"}"#);
}

#[test]
fn a_project_that_ships_its_own_identity_segment_is_not_given_a_second_one() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = crate::project::manifest_compiler::package_uuid_for("syw.probe.echo");
    let own = dir.path().join("own.json");
    std::fs::write(&own, br#"{"uuid":"mine","entity_type":"package"}"#).unwrap();
    let plan = plan_with(
        dir.path(),
        vec![],
        vec![NamespaceFile { relative: format!("{uuid}.json"), absolute: own }],
    );
    let out = dir.path().join("bundle.zip");
    bundle_writer_write(&plan, &out);

    // TWO ENTRIES WITH ONE NAME is a malformed archive, and which one a reader takes is
    // implementation-defined. A project that authored its own segment keeps it.
    let names = entries(&out);
    let matching = names.iter().filter(|n| *n == &format!("namespace/{uuid}.json")).count();
    assert_eq!(matching, 1, "got {names:?}");
    assert_eq!(read_entry(&out, &format!("namespace/{uuid}.json")), br#"{"uuid":"mine","entity_type":"package"}"#);
}

#[test]
fn a_written_bundle_reports_unsigned_and_its_real_size() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan_with(dir.path(), vec![artifact(dir.path(), "libecho.so", b"x")], vec![]);
    let out = dir.path().join("bundle.zip");
    let written = write_bundle(&plan, &out).unwrap();
    assert_eq!(written.framing, crate::publishing::types::Framing::Unsigned);
    assert_eq!(written.fqid, "syw.probe.echo");
    assert_eq!(written.version, "1.0.0");
    assert_eq!(written.size_bytes, std::fs::metadata(&out).unwrap().len());
}

#[test]
fn an_artifact_that_changes_mid_pack_is_refused_rather_than_shipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("libecho.so");
    std::fs::write(&path, b"four").unwrap();
    // The located size and the file no longer agree — which is what a build running concurrently with a
    // pack looks like. The archive it would produce matches NEITHER state, so it is refused.
    let located = LocatedArtifact {
        name: "libecho.so".into(),
        kind: "Dll".into(),
        entry_point: Some("wf_init".into()),
        absolute_path: path,
        size_bytes: 99,
    };
    let plan = plan_with(dir.path(), vec![located], vec![]);
    let e = write_bundle(&plan, &dir.path().join("bundle.zip")).unwrap_err().to_string();
    assert!(e.contains("changed while it was being packed"), "got {e}");
}

#[test]
fn read_identity_takes_the_fqid_from_the_BUNDLE_and_does_not_modify_it() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan_with(dir.path(), vec![], vec![]);
    let out = dir.path().join("bundle.zip");
    bundle_writer_write(&plan, &out);

    let before = std::fs::read(&out).unwrap();
    let identity = read_identity(&out).unwrap();
    assert_eq!(identity.fqid, "syw.probe.echo");
    assert_eq!(identity.version, "1.0.0");
    // NOTHING IS UNPACKED AND NOTHING IS REWRITTEN. A publish of a pre-built bundle uploads exactly
    // these bytes, and any modification here would invalidate a signature the bundle may already carry.
    assert_eq!(std::fs::read(&out).unwrap(), before, "reading an identity must not touch the file");
}

#[test]
fn an_archive_with_no_manifest_is_refused_as_not_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("random.zip");
    {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"not a bundle").unwrap();
        zip.finish().unwrap();
    }
    let e = read_identity(&path).unwrap_err().to_string();
    // The likely answer when someone points this at an arbitrary archive, and saying so beats a decode
    // error naming a field.
    assert!(e.contains("not a Waffler bundle"), "got {e}");
}

/// Write and unwrap, for the tests whose subject is the archive rather than the result.
fn bundle_writer_write(plan: &BundlePlan, out: &std::path::Path) {
    write_bundle(plan, out).unwrap();
}
