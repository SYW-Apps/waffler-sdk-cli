//! `sdk_cli::project-portal` — the project-side front door.
//!
//! Spec: `sdk_cli::project-portal` / `iproject-portal` / `project_portal_impl`.
//!
//! THESE FUNCTIONS ARE THE LIBRARY API. Argument parsing sits ABOVE them, in `main.rs`, which is
//! what lets the docker image build call `plan` directly and reach exactly what a developer at a
//! terminal reaches. A check written in a command handler instead of below here is a check the
//! library path silently skips — and the library path is the one our own image build uses, so the
//! skipped check would be skipped exactly where it matters most.

use std::path::Path;

use anyhow::Result;

use super::project_orchestrator;
use super::types::{BuildReport, BundlePlan, RenderedFile, Violation};

/// Produce a bundle plan for a project directory. Publishing's only question of Project.
pub fn plan(directory: &Path, skip_build: bool) -> Result<(BundlePlan, BuildReport)> {
    project_orchestrator::plan_bundle(directory, skip_build)
}

/// Check the manifest without building.
pub fn validate(directory: &Path) -> Result<Vec<Violation>> {
    project_orchestrator::validate_project(directory)
}

/// Build the project's declared artifacts.
pub fn build(directory: &Path) -> Result<BuildReport> {
    project_orchestrator::build_project(directory)
}

/// Create a new package project.
pub fn scaffold(
    directory: &Path,
    fqid: &str,
    version: &str,
    description: &str,
    sdk_path: &str,
    overwrite: bool,
) -> Result<Vec<RenderedFile>> {
    project_orchestrator::scaffold_project(directory, fqid, version, description, sdk_path, overwrite)
}
