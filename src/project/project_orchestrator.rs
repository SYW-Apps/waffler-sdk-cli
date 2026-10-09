//! `sdk_cli::project-orchestrator` — from a directory to a bundle plan.
//!
//! Spec: `sdk_cli::project-orchestrator` / `iproject-orchestrator` / `project_orchestrator_impl`.
//!
//! THE ORDER IS THE DESIGN, and it is the whole content of this module:
//!
//!   1. read the authoring manifest;
//!   2. VALIDATE IT — before anything expensive runs. A release build costs minutes and a malformed
//!      fqid costs microseconds to detect; the previous tool built first and validated at upload, so
//!      every manifest mistake was paid for at full price;
//!   3. build;
//!   4. LOCATE the artifacts the build was supposed to produce. After the build, so "it succeeded
//!      and produced nothing at the declared path" is a distinct, sayable failure rather than a
//!      stale artifact silently taking its place;
//!   5. compile the declarations.
//!
//! A FAILURE NAMES THE STAGE IT FAILED IN. "Invalid manifest", "build failed", "declared artifact
//! missing" and "declaration inconsistent" are four different problems with four different fixes,
//! and a tool that reports them as "pack failed" has converted a diagnosis into a search.

use std::path::Path;

use anyhow::{bail, Result};

use super::types::{BuildReport, BundlePlan, RenderedFile, Violation};
use super::{build_adapter, manifest_compiler, project_adapter, scaffold_specialist};

/// The whole workflow, in the order that makes each failure cheap.
pub fn plan_bundle(directory: &Path, skip_build: bool) -> Result<(BundlePlan, BuildReport)> {
    let authored = project_adapter::read_authored_manifest(directory)?;

    let violations = manifest_compiler::validate_authored(&authored);
    if !violations.is_empty() {
        bail!("{}", render_violations(directory, &violations));
    }

    let report = if skip_build {
        build_adapter::skipped()
    } else {
        match authored.build.as_ref() {
            Some(build) => {
                let manifest_path = directory.join(&build.manifest_path);
                let report = build_adapter::build_rust_crate(&manifest_path)?;
                if !report.succeeded {
                    // THE TOOLCHAIN'S OWN OUTPUT, UNCHANGED. It names a file, a line and a type; any
                    // sentence substituted for that is strictly less useful to whoever has to fix it.
                    bail!("the build failed.\n\n{}", report.output);
                }
                report
            }
            // A package with no build declaration is not an error: a project whose artifacts are
            // produced by something else entirely still packs, and refusing it would make this tool
            // the only way to build a Waffler package.
            None => build_adapter::skipped(),
        }
    };

    // EACH ARTIFACT IS RESOLVED BY EXACTLY ONE AUTHORITY, and which one is a property of the
    // declaration rather than a fallback chain. A declared `path` means the developer knows where the
    // file is; `fromBuild` means the build tool does, and it is asked. Nothing here searches.
    let mut located = Vec::with_capacity(authored.artifacts.len());
    for declared in &authored.artifacts {
        let (candidate, describe) = if declared.from_build {
            let Some(build) = authored.build.as_ref() else {
                bail!(
                    "an artifact declares `fromBuild` but {} declares no `build.manifestPath`, so there \
                     is no build to ask where its output went.",
                    super::types::MANIFEST_FILE
                );
            };
            let path = build_adapter::built_artifact_path(&directory.join(&build.manifest_path))?;
            let describe = format!("fromBuild ({})", path.display());
            (path, describe)
        } else {
            let path = declared.path.clone().unwrap_or_default();
            (directory.join(&path), path)
        };
        located.push(project_adapter::locate_artifact(declared, &candidate, &describe)?);
    }

    let namespace_files = project_adapter::read_namespace_tree(directory)?;
    // BEFORE ANYTHING IS BUILT OR WRITTEN. This also puts the refusal on `validate`, which stops
    // short of the build -- so the cheap command catches it, which is the command an author runs
    // while still editing.
    project_adapter::validate_namespace_tree(&namespace_files)?;
    let manifest_body = manifest_compiler::compile_manifest_body(&authored, &located);
    let package_uuid = manifest_compiler::package_uuid_for(&authored.fqid);

    Ok((
        BundlePlan {
            fqid: authored.fqid,
            version: authored.version,
            package_uuid,
            manifest_body,
            artifacts: located,
            namespace_files,
            project_directory: directory.to_path_buf(),
        },
        report,
    ))
}

/// The manifest checks and the namespace-tree check, with no build.
///
/// IT EXISTS SO THE CHEAP CHECK IS AVAILABLE ALONE. A tool whose only way to check a manifest is to
/// build the package teaches people not to check.
///
/// ITS DOC USED TO SAY "everything `plan_bundle` does up to the build", which was never true — it
/// does not locate artifacts and did not read the namespace tree — and the inaccuracy had a cost:
/// it is why the namespace-tree gate was first added only to `plan_bundle`, leaving the command an
/// author actually runs while editing silently exempt. A summary that overstates its subject sends
/// the next reader to the wrong call site.
pub fn validate_project(directory: &Path) -> Result<Vec<Violation>> {
    let authored = project_adapter::read_authored_manifest(directory)?;
    let mut violations = manifest_compiler::validate_authored(&authored);

    // AS A VIOLATION, NOT A BAIL. This command's contract is every problem at once, so a bad tree
    // must not hide the manifest problems found beside it. `plan_bundle` refuses outright instead,
    // which is right there: a pack either produces a bundle or does not.
    //
    // Same predicate either way — `waffler_shared::validate_staged_namespace_tree`, which the
    // INSTALLER also calls. Two call sites, two presentations, one rule.
    if let Err(e) = project_adapter::read_namespace_tree(directory)
        .and_then(|files| project_adapter::validate_namespace_tree(&files))
    {
        violations.push(Violation::new("namespace/", format!("{e:#}")));
    }

    Ok(violations)
}

/// Advice about the project's manifest that does not stop a pack — no build, like
/// [`validate_project`].
///
/// SEPARATE FROM `validate_project` RATHER THAN FOLDED INTO ITS ANSWER: a violation list and an
/// advisory list answer different questions, and a caller asking only whether the manifest is sound
/// must not have to filter one out of the other.
pub fn advise_project(directory: &Path) -> Result<Vec<super::types::Advisory>> {
    let authored = project_adapter::read_authored_manifest(directory)?;
    Ok(manifest_compiler::advise_authored(&authored))
}

/// Build what the project declares, in the profile a publishable bundle requires.
pub fn build_project(directory: &Path) -> Result<BuildReport> {
    let authored = project_adapter::read_authored_manifest(directory)?;
    let Some(build) = authored.build.as_ref() else {
        bail!(
            "{} declares no `build.manifestPath`, so there is nothing for this command to build.\n\
             Add one, or build the declared artifacts with your own tooling and use `waffler pack --no-build`.",
            super::types::MANIFEST_FILE
        );
    };
    build_adapter::build_rust_crate(&directory.join(&build.manifest_path))
}

/// Create a new package project on disk.
pub fn scaffold_project(
    directory: &Path,
    fqid: &str,
    version: &str,
    description: &str,
    sdk_path: &str,
    overwrite: bool,
) -> Result<Vec<RenderedFile>> {
    let files = scaffold_specialist::render_project(fqid, version, description, sdk_path);
    project_adapter::write_project_files(directory, &files, overwrite)?;
    Ok(files)
}

/// Every violation, in one message, with the file named.
///
/// ONE MESSAGE RATHER THAN THE FIRST FAILURE. Fixing a manifest one refused field per build is how a
/// five-minute correction becomes an afternoon.
fn render_violations(directory: &Path, violations: &[Violation]) -> String {
    let mut out = format!(
        "{} has {} problem{}:\n",
        directory.join(super::types::MANIFEST_FILE).display(),
        violations.len(),
        if violations.len() == 1 { "" } else { "s" }
    );
    for v in violations {
        out.push_str(&format!("  {v}\n"));
    }
    out
}

#[cfg(test)]
#[path = "project_orchestrator.test.rs"]
mod tests;
