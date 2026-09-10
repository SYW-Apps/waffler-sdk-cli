//! `sdk_cli::publish-portal` — the `pack`, `publish` and `unpublish` front door.
//!
//! Spec: `sdk_cli::publish-portal` / `ipublish-portal` / `publish_portal_impl`.
//!
//! OUTPUT NAMES THE REGISTRY AND THE SHAPE. Every run says which registry it resolved, how it
//! resolved it, and whether the artifact carries a publisher signature. Those three facts are what a
//! developer needs to know they did what they meant to, and all three were unavailable in the
//! previous tool.

use std::path::Path;

use anyhow::Result;

use super::publish_orchestrator;
use super::types::{PublishOutcome, WrittenBundle};
use crate::project::types::BuildReport;
use crate::session::types::{PersistedSession, TargetRegistry};

/// Build a bundle and stop. No network, no credential, nothing uploaded.
pub fn pack(
    directory: &Path,
    output_path: Option<&Path>,
    skip_build: bool,
    publisher_key_path: Option<&Path>,
) -> Result<(WrittenBundle, BuildReport)> {
    publish_orchestrator::pack(directory, output_path, skip_build, publisher_key_path)
}

/// Create a publisher signing key.
pub fn new_publisher_key(path: &Path) -> Result<[u8; 32]> {
    publish_orchestrator::new_publisher_key(path)
}

/// Build a bundle if needed and upload it to the resolved registry.
pub async fn publish(
    client: &reqwest::Client,
    session: &PersistedSession,
    directory: &Path,
    bundle_path: Option<&Path>,
    registry_flag: Option<&str>,
    skip_build: bool,
    publisher_key_path: Option<&Path>,
) -> Result<PublishOutcome> {
    publish_orchestrator::publish(client, session, directory, bundle_path, registry_flag, skip_build, publisher_key_path)
        .await
}

/// Withdraw a published version.
pub async fn unpublish(
    client: &reqwest::Client,
    session: &PersistedSession,
    fqid: &str,
    version: &str,
    registry_flag: Option<&str>,
) -> Result<TargetRegistry> {
    publish_orchestrator::unpublish(client, session, fqid, version, registry_flag).await
}
