//! `sdk_cli::session-portal` — the session front door.
//!
//! Spec: `sdk_cli::session-portal` / `isession-portal` / `session_portal_impl`.
//!
//! LIKE THE OTHER PORTALS, THESE FUNCTIONS ARE THE LIBRARY API and argument parsing sits above them.

use anyhow::Result;

use super::session_orchestrator;
use super::types::{PersistedSession, RegistryCredential, RegistryProfile, TargetRegistry};
use super::session_store;

/// Read the persisted session once, at the start of a command.
///
/// HYDRATION, AND THE ONLY REASON THE STORE IS DURABLE. Every command needs the `use` default —
/// including one that only reports which registry it resolved — and a credential that was not
/// hydrated is a browser login the developer already performed being asked for again.
pub fn hydrate() -> Result<PersistedSession> {
    session_store::load()
}

/// Which registry does this invocation address, and why.
pub fn resolve(session: &PersistedSession, flag_value: Option<&str>) -> Result<TargetRegistry> {
    session_orchestrator::resolve_registry(session, flag_value)
}

/// A usable access token for a resolved registry.
pub async fn bearer(
    client: &reqwest::Client,
    session: &PersistedSession,
    registry: &TargetRegistry,
) -> Result<String> {
    session_orchestrator::bearer_for(client, session, registry).await
}

/// What the registry says about itself.
pub async fn profile(client: &reqwest::Client, registry: &TargetRegistry) -> Result<RegistryProfile> {
    session_orchestrator::profile_of(client, registry).await
}

/// Sign in to a registry, against whatever issuer that registry advertises.
pub async fn login(
    client: &reqwest::Client,
    session: &PersistedSession,
    flag_value: Option<&str>,
) -> Result<RegistryCredential> {
    let registry = resolve(session, flag_value)?;
    // REPORTED BEFORE THE BROWSER OPENS. A login that silently targeted the wrong registry produces a
    // credential filed under a name the developer did not intend, and they find out at publish time.
    println!("Signing in to {} ({})", registry.base_url, registry.source.because());
    session_orchestrator::sign_in(client, &registry).await
}

/// Sign out of one registry, leaving every other session intact.
pub fn logout(session: &PersistedSession, flag_value: Option<&str>) -> Result<(TargetRegistry, bool)> {
    let registry = resolve(session, flag_value)?;
    let existed = session_orchestrator::sign_out(&registry)?;
    Ok((registry, existed))
}

/// Report the identity held for a registry, naming the registry it is for.
pub fn whoami(
    session: &PersistedSession,
    flag_value: Option<&str>,
) -> Result<(TargetRegistry, Option<RegistryCredential>)> {
    let registry = resolve(session, flag_value)?;
    let credential = session_orchestrator::who_am_i(session, &registry);
    Ok((registry, credential))
}

/// Persist a registry as the default, and say whether there is a credential for it.
pub fn use_registry(session: &PersistedSession, base_url: &str) -> Result<(TargetRegistry, bool)> {
    session_orchestrator::set_default(session, base_url)
}
