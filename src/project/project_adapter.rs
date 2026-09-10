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

/// Resolve one declared artifact to an absolute path and measure it.
pub fn locate_artifact(directory: &Path, declared: &DeclaredArtifact) -> Result<LocatedArtifact> {
    let joined = directory.join(&declared.path);
    // NO CANDIDATE LIST, NO FALLBACK DIRECTORY, NO SEARCH.
    //
    // The previous tool tried `target/release`, then the wasm target dir, then the project root,
    // and took the first hit — so a stale artifact from a build the developer had forgotten was as
    // good as a fresh one, and nothing could report that it had happened.
    let absolute = joined.canonicalize().map_err(|_| {
        // BOTH SPELLINGS IN THE MESSAGE. A relative path that looks right and a working directory
        // that is not what the developer thinks are the same mistake wearing different clothes, and
        // only printing both tells them which one they have.
        anyhow::anyhow!(
            "declared artifact '{}' does not exist.\n  resolved to: {}\n  \
             (nothing else is searched: the declared path is the path)",
            declared.path,
            joined.display()
        )
    })?;

    let meta = std::fs::metadata(&absolute).with_context(|| format!("reading {}", absolute.display()))?;
    if !meta.is_file() {
        bail!("declared artifact '{}' resolves to {}, which is not a file", declared.path, absolute.display());
    }

    Ok(LocatedArtifact {
        name: declared.bundle_name(),
        kind: declared.kind.clone(),
        entry_point: declared.entry_point.clone(),
        absolute_path: absolute,
        // Measured here so an oversized bundle can be refused before an upload rather than during
        // one.
        size_bytes: meta.len(),
    })
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
