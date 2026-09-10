//! `sdk_cli::registry-metadata-adapter` — asking a registry what it is.
//!
//! Spec: `sdk_cli::registry-metadata-adapter` / `iregistry-metadata-adapter` /
//! `registry_metadata_adapter_impl`.
//!
//! AN UNREACHABLE OR UNPARSEABLE REGISTRY IS A REFUSAL NAMING THE URL, never a fallback to defaults.
//! Defaulting to "no authentication required" on a failed probe is how a tool sends an
//! unauthenticated publish to a registry that would have taken a credential, and then reports the
//! 401 as the registry's fault.

use anyhow::{bail, Context, Result};

use super::types::RegistryProfile;

/// The metadata route. One place, so it is a fact rather than a recollection.
pub const METADATA_PATH: &str = "/v1/registry";

/// Read a registry's self-description.
pub async fn fetch_profile(client: &reqwest::Client, base_url: &str) -> Result<RegistryProfile> {
    let url = format!("{base_url}{METADATA_PATH}");
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        bail!("{url} answered {status}: {}", body.trim());
    }

    // HTML IS A SPECIFIC FAILURE WORTH NAMING.
    //
    // A registry serving a console behind a single-page-app fallback answers an unknown API path
    // with its index, so a client on a stale or wrong base URL gets 200 and markup — and finds out
    // only when decoding fails somewhere unrelated. The likely cause is a wrong base URL rather than
    // a broken registry, and the message should say which to look at.
    if content_type.contains("text/html") || body.trim_start().starts_with('<') {
        bail!(
            "{url} answered with HTML rather than JSON. That is what a Waffler registry's web console \
             does for a path it does not recognise, so the base URL is probably wrong — it should be \
             the registry root, with no path and no trailing slash."
        );
    }

    // A RESPONSE THAT SAYS AUTHENTICATION IS AVAILABLE BUT NAMES NO ISSUER IS DECODED FAITHFULLY
    // rather than repaired. The absence is a real fact about that deployment, and the login flow
    // refuses on it with a message about the registry not saying — never by substituting a default,
    // which would authenticate a developer against someone else's identity provider.
    serde_json::from_str(&body).with_context(|| format!("{url} did not answer with registry metadata"))
}
