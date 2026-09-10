//! `sdk_cli::project-client-adapter` — Publishing's one door into Project.
//!
//! Spec: `sdk_cli::project-client-adapter` / `iproject-client-adapter` /
//! `project_client_adapter_impl`.
//!
//! IT IS THIN, AND SAYING SO IS BETTER THAN DRESSING IT UP. Both subsystems are modules of one
//! binary, so this is a function call and the boundary it crosses is conceptual rather than physical.
//! What it buys is that the crossing happens in ONE place: when the plan's shape changes, exactly one
//! file in Publishing has to know, and the orchestrator that decides publishing ORDER does not become
//! a second reader of the project model.
//!
//! IT NEVER BUILDS AND NEVER WRITES. Everything it can do is ask Project a question; a component in
//! Publishing that could invoke a build toolchain would be a second build path, and the second one is
//! the one nobody keeps current.

use std::path::Path;

use anyhow::Result;

use crate::project::project_portal;
use crate::project::types::{BuildReport, BundlePlan};

/// Ask Project for a bundle plan.
pub fn plan_from_directory(directory: &Path, skip_build: bool) -> Result<(BundlePlan, BuildReport)> {
    project_portal::plan(directory, skip_build)
}
