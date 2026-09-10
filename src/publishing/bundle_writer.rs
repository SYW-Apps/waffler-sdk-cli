//! `sdk_cli::bundle-writer` — the one place in this tool that knows the bundle format.
//!
//! Spec: `sdk_cli::bundle-writer` / `ibundle-writer` / `bundle_writer_impl`.
//!
//! ## THE LAYOUT
//!
//! ```text
//!   .manifest                  an InstalledPackage JSON: identity, hosting mode, declarations,
//!                              and an artifacts array whose hashes are measured here
//!   artifact/<name>            each embedded artifact, byte for byte
//!   namespace/<uuid>.json      the package identity segment
//!   namespace/<...>            the project's own entity files, unchanged
//! ```
//!
//! NOT `package.json` AT THE ROOT — that is the layout the previous tool wrote, and
//! `registry::publication::bundle-reader` refuses it by explicit test. The reader is the other half
//! of this component and it already exists; where the two disagree, the reader is right and this is
//! the bug.
//!
//! IT NEVER RE-ZIPS AN EXISTING BUNDLE. A bundle is written once, from parts, and thereafter treated
//! as opaque bytes — re-compressing or normalising after the fact destroys a signature over the whole
//! payload and produces an artifact core refuses as IntegrityFailure, on a stranger's machine.

use std::io::{Read, Seek, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use sha2::Digest;

use super::types::{Framing, WrittenBundle};
use crate::project::types::BundlePlan;

/// The manifest's entry name, at the archive root.
pub const MANIFEST_ENTRY: &str = ".manifest";
/// The directory embedded artifacts live under.
pub const ARTIFACT_PREFIX: &str = "artifact";
/// The directory entity segments live under.
pub const NAMESPACE_PREFIX: &str = "namespace";

/// How much of an artifact is read at a time while hashing and writing it.
///
/// STREAMED RATHER THAN BUFFERED, because a package artifact may be a hundred megabytes and holding
/// one whole buys nothing — the hash is a rolling computation and the archive writer takes bytes as
/// they come.
const CHUNK: usize = 64 * 1024;

/// Write `.manifest`, `artifact/<name>` for each artifact, and the namespace tree.
pub fn write_bundle(plan: &BundlePlan, output_path: &Path) -> Result<WrittenBundle> {
    if let Some(parent) = output_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let file = std::fs::File::create(output_path)
        .with_context(|| format!("creating {}", output_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);

    // STORED, NOT DEFLATED, for two reasons.
    //
    // Nothing downstream may depend on a compression path having been taken. And a stored entry's
    // DECLARED uncompressed size is its real size — the registry refuses a metadata entry that
    // expands past its limit, and a compressed entry can declare one size and expand to another,
    // which is a decompression bomb that was accepted and published before the reader learned to
    // check both numbers.
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    // The artifacts go in first so their hashes exist before the manifest that declares them is
    // written. The alternative — writing the manifest first and patching it — would mean rewriting
    // part of an archive after the fact, which is the operation this component exists not to do.
    let mut artifact_refs = Vec::with_capacity(plan.artifacts.len());
    for artifact in &plan.artifacts {
        let entry = format!("{ARTIFACT_PREFIX}/{}", artifact.name);
        zip.start_file(&entry, options).with_context(|| format!("starting {entry}"))?;

        let mut source = std::fs::File::open(&artifact.absolute_path)
            .with_context(|| format!("reading {}", artifact.absolute_path.display()))?;
        let mut hasher = sha2::Sha256::new();
        let mut buffer = vec![0u8; CHUNK];
        let mut written: u64 = 0;
        loop {
            let read = source.read(&mut buffer).with_context(|| format!("reading {}", artifact.absolute_path.display()))?;
            if read == 0 {
                break;
            }
            // ARTIFACT HASHES ARE COMPUTED HERE, NEVER COPIED. A hash carried in from a manifest a
            // developer wrote is a claim; a hash computed over the bytes being written is a fact, and
            // the two disagreeing is exactly what a content hash exists to detect. Hashing the same
            // buffer that is written means the two cannot describe different bytes.
            hasher.update(&buffer[..read]);
            zip.write_all(&buffer[..read]).with_context(|| format!("writing {entry}"))?;
            written += read as u64;
        }

        artifact_refs.push(serde_json::json!({
            "name": artifact.name,
            "kind": artifact.kind,
            "hash": hex::encode(hasher.finalize()),
            "entry_point": artifact.entry_point,
        }));

        // Measured at location time and again here. A file that changed between the two is a build
        // running concurrently with a pack, and the bundle it produces is neither of the two states.
        if written != artifact.size_bytes {
            bail!(
                "{} changed while it was being packed ({} bytes when located, {written} when written). \
                 Refusing rather than shipping an archive that matches neither state.",
                artifact.absolute_path.display(),
                artifact.size_bytes
            );
        }
    }

    // The manifest body arrives compiled, missing only this.
    let mut manifest = plan.manifest_body.clone();
    manifest["artifacts"] = serde_json::Value::Array(artifact_refs);
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).context("encoding the manifest")?;
    zip.start_file(MANIFEST_ENTRY, options).context("starting .manifest")?;
    zip.write_all(&manifest_bytes).context("writing .manifest")?;

    for file in &plan.namespace_files {
        let entry = format!("{NAMESPACE_PREFIX}/{}", file.relative);
        zip.start_file(&entry, options).with_context(|| format!("starting {entry}"))?;
        let bytes = std::fs::read(&file.absolute)
            .with_context(|| format!("reading {}", file.absolute.display()))?;
        zip.write_all(&bytes).with_context(|| format!("writing {entry}"))?;
    }

    // THE PACKAGE IDENTITY SEGMENT. Without it the staged reader finds no segment and INGESTS
    // NOTHING — the package installs as a record with no entity behind it, which looks like a
    // successful install until something tries to reference it.
    let segment_entry = format!("{NAMESPACE_PREFIX}/{}.json", plan.package_uuid);
    if !plan.namespace_files.iter().any(|f| f.relative == format!("{}.json", plan.package_uuid)) {
        zip.start_file(&segment_entry, options).with_context(|| format!("starting {segment_entry}"))?;
        zip.write_all(&identity_segment(plan)?).with_context(|| format!("writing {segment_entry}"))?;
    }

    zip.finish().context("finalising the archive")?;

    let size_bytes = std::fs::metadata(output_path)
        .with_context(|| format!("measuring {}", output_path.display()))?
        .len();

    Ok(WrittenBundle {
        path: output_path.to_path_buf(),
        fqid: plan.fqid.clone(),
        version: plan.version.clone(),
        // NOTHING ELSE TOUCHES THESE BYTES. The invariant is that the bytes a node verifies are the
        // bytes that were signed, and a later re-zip destroys that while looking harmless.
        framing: Framing::Unsigned,
        size_bytes,
    })
}

/// The `namespace/<uuid>.json` identity segment.
///
/// THE LENIENT StagedSegment SHAPE: only `uuid` and `entity_type` are required by the reader, and the
/// rest is what makes the ingested entity legible to a human looking at it.
fn identity_segment(plan: &BundlePlan) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "uuid": plan.package_uuid,
        "technical_name": plan.fqid,
        "display_name": plan.fqid,
        "entity_type": "package",
        // The instant the bundle was built. A fixed literal here would make every package claim the
        // same creation time, which is worse than useless in a catalog sorted by it.
        "created_at": chrono::Utc::now().to_rfc3339(),
        "tags": [],
        "parent_uuid": null,
    }))
    .context("encoding the package identity segment")
}

/// Read the fqid and version out of a bundle that already exists.
///
/// NOTHING IS UNPACKED AND NOTHING IS REWRITTEN. A publish of a pre-built bundle uploads exactly
/// these bytes, and any modification here would invalidate a signature the bundle may already carry.
pub fn read_identity(bundle_path: &Path) -> Result<WrittenBundle> {
    let file = std::fs::File::open(bundle_path)
        .with_context(|| format!("reading {}", bundle_path.display()))?;
    let size_bytes = file.metadata().with_context(|| format!("measuring {}", bundle_path.display()))?.len();

    // A signed bundle's trailing bytes are not part of the archive, and `zip::ZipArchive` locates the
    // central directory by scanning back from the end — so it reads a trailer-bearing file fine. What
    // it must never do is write.
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .with_context(|| format!("{} is not a readable archive", bundle_path.display()))?;

    let mut text = String::new();
    match archive.by_name(MANIFEST_ENTRY) {
        Ok(mut entry) => {
            entry.read_to_string(&mut text).context("reading .manifest")?;
        }
        Err(_) => {
            // The likely answer when someone points this at an arbitrary archive, and saying so beats
            // a decode error naming a field.
            bail!(
                "{} carries no /{MANIFEST_ENTRY}, so it is not a Waffler bundle.",
                bundle_path.display()
            );
        }
    }

    let manifest: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("the /{MANIFEST_ENTRY} in {} is not valid JSON", bundle_path.display()))?;

    // THE IDENTITY COMES FROM THE BUNDLE, NOT FROM A PROJECT BESIDE IT. Those two can disagree, and
    // the bundle is the thing being uploaded — reading a manifest from the working directory to
    // describe an archive built last week is how a package gets published under the wrong name,
    // successfully.
    let fqid = manifest.get("fqid").and_then(|v| v.as_str()).with_context(|| {
        format!("the /{MANIFEST_ENTRY} in {} names no fqid", bundle_path.display())
    })?;
    let version = manifest.get("version").and_then(|v| v.as_str()).with_context(|| {
        format!("the /{MANIFEST_ENTRY} in {} names no version", bundle_path.display())
    })?;

    Ok(WrittenBundle {
        path: bundle_path.to_path_buf(),
        fqid: fqid.to_string(),
        version: version.to_string(),
        // Left to the signature specialist, which is the component that knows how to read a suffix
        // safely. A guess here would be a second boundary parser, and the two disagreeing is exactly
        // the exploit the careful one exists to close.
        framing: Framing::Unsigned,
        size_bytes,
    })
}

/// Reopen a bundle for reading without holding it. Used by the framing detector.
pub fn open_for_reading(path: &Path) -> Result<impl Read + Seek> {
    std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
#[path = "bundle_writer.test.rs"]
mod tests;
