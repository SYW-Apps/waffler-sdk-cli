//! `sdk_cli::session-client-adapter` — Publishing's one door into Session.
//!
//! Spec: `sdk_cli::session-client-adapter` / `isession-client-adapter` /
//! `session_client_adapter_impl`.
//!
//! IT RESOLVES ONCE PER COMMAND, AND THAT IS ITS REAL JOB rather than a caching nicety. Registry
//! resolution reads a flag, an environment variable, a persisted default and a built-in; if the
//! orchestrator asked twice — once to report the target and again to upload — a concurrent `use` or a
//! differently-read environment could answer differently, and the registry NAMED in the output would
//! not be the registry PUBLISHED to. Resolving once and carrying the answer makes "we told you where
//! this was going" a fact instead of a hope.
//!
//! SESSION IS TOLD NOTHING ABOUT WHAT IS BEING PUBLISHED. No fqid, no version, no size. Session
//! resolving an address it cannot see the contents of is what makes it impossible for a change in what
//! a bundle contains to change who the tool authenticates as.

use anyhow::Result;

use crate::session::session_portal;
use crate::session::types::{PersistedSession, RegistryProfile, TargetRegistry};

/// Which registry this command addresses, and which source said so.
///
/// CALLED ONCE, BEFORE ANYTHING IRREVERSIBLE, so the answer can be reported while it is still cheap
/// to change course.
pub fn target(session: &PersistedSession, flag_value: Option<&str>) -> Result<TargetRegistry> {
    session_portal::resolve(session, flag_value)
}

/// What that registry accepts and whether it can publish at all.
///
/// READ BEFORE A BUILD, because "this instance only serves downloads" is worth the one request that
/// saves the build.
pub async fn describe(client: &reqwest::Client, registry: &TargetRegistry) -> Result<RegistryProfile> {
    session_portal::profile(client, registry).await
}

/// A usable access token, fetched LATE.
///
/// A token checked before a long build and used after it can expire in between, which is why this is a
/// separate call rather than a field on the target.
pub async fn bearer(
    client: &reqwest::Client,
    session: &PersistedSession,
    registry: &TargetRegistry,
) -> Result<String> {
    session_portal::bearer(client, session, registry).await
}
