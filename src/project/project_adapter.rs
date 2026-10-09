//! `sdk_cli::project-adapter` — what is on disk in a package project.
//!
//! Spec: `sdk_cli::project-adapter` / `iproject-adapter` / `project_adapter_impl`.
//!
//! EVERY METHOD EITHER RESOLVES A NAMED PATH OR FAILS NAMING IT. Nothing here falls back to a
//! candidate list, a sibling directory or a previous build's output: a path that does not resolve
//! is a sentence a developer can act on, and a path that resolved somewhere unexpected is a bundle
//! that ships the wrong binary and cannot say so.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::types::{
    AuthoredPackage, DeclaredArtifact, LocatedArtifact, NamespaceFile, RenderedFile,
    LEGACY_MANIFEST_FILE, MANIFEST_FILE,
};

/// Read and parse `waffler.json` from a project directory.
pub fn read_authored_manifest(directory: &Path) -> Result<AuthoredPackage> {
    let path = directory.join(MANIFEST_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // FINDING A LEGACY MANIFEST IS REPORTED AS EXACTLY THAT, naming both files.
            //
            // Without this the message is "waffler.json not found", and the developer's next move
            // is to rename the file they do have — which produces a manifest that parses (every
            // field it lacks is defaulted) and is wrong in every field. The rename is the trap, so
            // the message has to close it before they think of it.
            if directory.join(LEGACY_MANIFEST_FILE).exists() {
                bail!(
                    "{} has no {MANIFEST_FILE}, but it does have a {LEGACY_MANIFEST_FILE}.\n\
                     That is the previous manifest model, and it is not this one: `namespace` became \
                     `fqid`, `module.runtime` became `hosting_mode`, `permissions` became \
                     `permission_groups`, and artifacts gained content hashes that did not exist \
                     before. Renaming the file would produce a manifest that parses and is wrong in \
                     every field — write a new {MANIFEST_FILE} instead (`waffler scaffold` shows the \
                     shape).",
                    directory.display()
                );
            }
            bail!("no {MANIFEST_FILE} in {}", directory.display());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    // Serde's own line and column, kept. A tool that says only "invalid manifest" for a JSON syntax
    // error has thrown away the part that locates it.
    serde_json::from_str(&text).with_context(|| format!("{} is not a valid {MANIFEST_FILE}", path.display()))
}

/// Confirm one artifact exists at an already-resolved candidate path, and measure it.
///
/// `candidate` came from exactly one of two authorities and from no third: the declaration's own
/// `path`, joined to the project; or the build tool, asked where it put its output. NO CANDIDATE LIST,
/// NO FALLBACK DIRECTORY, NO SEARCH — the previous tool tried the release target dir, then the wasm
/// target dir, then the project root, and took the first hit, so a stale artifact from a build the
/// developer had forgotten was as good as a fresh one with nothing able to report that it happened.
///
/// `describe` is how the candidate is spelled in a refusal: the declared path when a developer wrote
/// one, and where the build tool said its output would be when they did not.
pub fn locate_artifact(
    declared: &DeclaredArtifact,
    candidate: &Path,
    describe: &str,
) -> Result<LocatedArtifact> {
    let absolute = candidate.canonicalize().map_err(|_| {
        // BOTH SPELLINGS IN THE MESSAGE. A relative path that looks right and a working directory that
        // is not what the developer thinks are the same mistake wearing different clothes, and only
        // printing both tells them which one they have.
        anyhow::anyhow!(
            "declared artifact '{describe}' does not exist.\n  resolved to: {}\n  \
             (nothing else is searched: one authority answers, and this is its answer)",
            candidate.display()
        )
    })?;

    let meta = std::fs::metadata(&absolute).with_context(|| format!("reading {}", absolute.display()))?;
    if !meta.is_file() {
        // `canonicalize` SUCCEEDS on a directory, so without this the failure would surface inside the
        // archive writer as an error about reading bytes.
        bail!("declared artifact '{describe}' resolves to {}, which is not a file", absolute.display());
    }

    Ok(LocatedArtifact {
        name: declared.bundle_name(&absolute),
        kind: declared.kind.clone(),
        entry_point: declared.entry_point.clone(),
        absolute_path: absolute,
        // Measured here so an oversized bundle can be refused before an upload rather than during one.
        size_bytes: meta.len(),
    })
}

/// Refuse a `namespace/` tree the installer will refuse, at PACK time.
///
/// ## Why this is a call and not a check
///
/// The rule is core's: Waffler content must sit under a crate root (the crate manifest is what
/// carries `exports`, so crate-less content is reachable by every consumer with nothing able to
/// mark it internal), and two segments sharing a technical_name under one parent both mount and
/// then race for the same derived FQID.
///
/// `read_namespace_tree` above refuses to parse these files, and for a reason that still holds:
/// the segment format belongs to VFS's staged reader. So this does not parse the format either —
/// it decodes with SHARED'S OWN TYPE and hands the result to SHARED'S OWN PREDICATE.
/// `vfs::staged_package_reader` calls the same function, so there is one rule with two callers
/// rather than a packer's opinion about an installer's invariant.
///
/// ## ONE ENTRY POINT, AND DO NOT REACH PAST IT
///
/// `validate_staged_namespace_tree` runs crate-root AND sibling-uniqueness together, and the halves
/// are private on purpose. A packer that pre-checked HALF of what the installer enforces would be
/// worse than one that pre-checked nothing: it teaches an author that packing clean means
/// installing clean, which is exactly the belief this function exists to make true.
///
/// ## A file that cannot be decoded is a REFUSAL
///
/// Every field but `uuid` and `entity_type` is defaulted so an older bundle still decodes, which
/// means a file failing to decode is missing one of those two — not merely old. Skipping it would
/// hand the predicate a SHORTER tree than the bundle ships and call the result a pass, so the
/// refusal names the file.
pub fn validate_namespace_tree(files: &[NamespaceFile]) -> Result<()> {
    if files.is_empty() {
        // A package need not ship entities; a pure native-DLL pack has nothing to check.
        return Ok(());
    }

    let mut segments = Vec::with_capacity(files.len());
    for file in files {
        let bytes = std::fs::read(&file.absolute)
            .with_context(|| format!("reading the namespace segment {}", file.relative))?;
        let segment: waffler_shared::StagedSegment = serde_json::from_slice(&bytes).map_err(|e| {
            anyhow::anyhow!(
                "namespace/{} is not a readable entity segment: {e}. Every field but `uuid` and \
                 `entity_type` is optional, so this is missing one of those two rather than being \
                 an older format",
                file.relative
            )
        })?;
        segments.push(segment);
    }

    // THE WHOLE TREE AT ONCE, never per file: sibling uniqueness is a property of the SET, and a
    // per-file loop could not see it.
    waffler_shared::validate_staged_namespace_tree(&segments).map_err(|e| {
        // Core's message names the offending segment, its entity_type and what it costs. Wrapped
        // rather than replaced, with the one thing it cannot know: that this is a PACK refusing
        // early on behalf of an install, so the author does not go looking for a local
        // misconfiguration.
        anyhow::anyhow!(
            "this package's namespace/ tree would be refused when it is INSTALLED, so it is \
             refused here instead: {}",
            e.message
        )
    })?;
    Ok(())
}

/// Collect the project's `namespace/` entity files.
///
/// READ, NOT PARSED. The segment format belongs to VFS's staged reader; a packer that parsed and
/// re-emitted it would be a second implementation of a format it does not own, and it would be the
/// one that drifts.
pub fn read_namespace_tree(directory: &Path) -> Result<Vec<NamespaceFile>> {
    let root = directory.join("namespace");
    if !root.exists() {
        // AN ABSENT `namespace/` IS AN EMPTY RESULT. A package need not ship entities, and turning
        // that into an error would make the common case explain itself.
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&root).into_iter().filter_map(std::result::Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            // FORWARD SLASHES REGARDLESS OF PLATFORM. A zip entry named with a backslash is a zip
            // entry whose path component is a filename on every reader that matters, so a bundle
            // packed on Windows would carry entries no Linux node can find.
            .replace('\\', "/");
        files.push(NamespaceFile { relative, absolute: entry.path().to_path_buf() });
    }
    // Sorted so a bundle packed twice from one tree has its entries in one order. Directory walk
    // order is filesystem-dependent, and a bundle whose entry order varies is one whose bytes vary
    // for no reason anybody chose.
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

/// Write a rendered set of files into a directory, creating parents.
pub fn write_project_files(directory: &Path, files: &[RenderedFile], overwrite: bool) -> Result<()> {
    // THE REFUSAL COMES FIRST, before any file is written.
    //
    // A scaffold that checked per-file would leave half a project behind on the file that collided,
    // which is worse than either outcome it was choosing between.
    if !overwrite && directory.exists() {
        let mut entries = std::fs::read_dir(directory)
            .with_context(|| format!("reading {}", directory.display()))?;
        if entries.next().is_some() {
            bail!(
                "{} is not empty. Pass --overwrite to write into it anyway.",
                directory.display()
            );
        }
    }

    for file in files {
        let target = directory.join(&file.relative_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&target, &file.contents)
            .with_context(|| format!("writing {}", target.display()))?;
    }
    Ok(())
}

/// Where a bundle is written when no output path is given.
pub fn default_bundle_path(directory: &Path, fqid: &str) -> PathBuf {
    directory.join(format!("{fqid}.zip"))
}

#[cfg(test)]
#[path = "project_adapter.test.rs"]
mod tests;
