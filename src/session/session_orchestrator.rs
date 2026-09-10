//! `sdk_cli::session-orchestrator` — which registry, and who am I on it.
//!
//! Spec: `sdk_cli::session-orchestrator` / `isession-orchestrator` / `session_orchestrator_impl`.
//!
//! IT DECIDES NOTHING ABOUT AUTHORIZATION. Whether an identity may publish under a namespace is the
//! registry's answer, surfaced as the registry worded it. The previous tool checked namespace
//! ownership locally against a cached account record and refused publishes the registry would have
//! accepted — a client disagreeing with the server about permission, in the merely obstructive
//! direction until the day the cache is stale in the other one.

use anyhow::{bail, Context, Result};

use super::types::{
    PersistedSession, RegistryCredential, RegistryProfile, RegistrySource, TargetRegistry,
};
use super::{oidc_adapter, registry_metadata_adapter, session_store};

/// The registry a command addresses when nothing else says otherwise.
///
/// LAST IN THE ORDER, NOT FIRST. A compiled-in registry as the primary answer is exactly the
/// assumption this subsystem exists to remove — the previous tool had two hosts compiled in and so
/// could not address a private or self-hosted registry at all, which is the ordinary case for this
/// product rather than an advanced one.
pub const BUILTIN_REGISTRY: &str = "https://registry.waffler.dev";

/// The environment variable that names a registry.
pub const ENV_REGISTRY: &str = "WAFFLER_REGISTRY";

/// The environment variable that carries a bearer directly.
///
/// IT EXISTS BECAUSE CI HAS NO BROWSER. An automated publisher cannot run an interactive
/// authorization-code flow, and a tool that requires one is a tool people work around by writing a
/// token into the credential file with a script — which is strictly worse, because it persists.
pub const ENV_TOKEN: &str = "WAFFLER_REGISTRY_TOKEN";

/// The public OAuth client this tool identifies as when a registry names none.
///
/// A FALLBACK FOR THE CLIENT ID ONLY, NEVER FOR THE PROVIDER. A client id is a public identifier that
/// the provider either recognises or rejects — guessing it wrong produces a clean refusal from the
/// right party. Guessing a DISCOVERY URL wrong authenticates the developer against someone else's
/// identity provider, which is why that one is refused rather than defaulted.
pub const DEFAULT_CLIENT_ID: &str = "waffler-cli";

/// How close to expiry a token may be and still be handed out.
///
/// A token that expires between the check and a hundred-megabyte upload wastes the upload;
/// refreshing on a margin costs one request.
fn refresh_margin() -> chrono::Duration {
    chrono::Duration::seconds(120)
}

/// Decide which registry this invocation addresses, and record which source said so.
pub fn resolve_registry(session: &PersistedSession, flag_value: Option<&str>) -> Result<TargetRegistry> {
    // ORDER: flag, environment, persisted default, built-in. Four sources means four ways to be
    // surprised, so the SOURCE is returned alongside the URL and reported on every command.
    let (raw, source) = if let Some(v) = flag_value.map(str::trim).filter(|v| !v.is_empty()) {
        (v.to_string(), RegistrySource::Flag)
    } else if let Some(v) = std::env::var(ENV_REGISTRY).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        (v, RegistrySource::Environment)
    } else if let Some(v) = session_store::default_registry(session) {
        (v, RegistrySource::Default)
    } else {
        (BUILTIN_REGISTRY.to_string(), RegistrySource::Builtin)
    };

    Ok(TargetRegistry { base_url: normalise_registry_url(&raw)?, source })
}

/// Reduce a registry URL to the one spelling everything keys on.
///
/// ONCE, HERE. The credential store keys on exactly this string; a second normalisation anywhere
/// would let two spellings of one registry hold two credentials and pick whichever the current
/// command's spelling found — and the request that went to the wrong one would carry a valid bearer
/// for somewhere else.
pub fn normalise_registry_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let Some((scheme, rest)) = raw.split_once("://") else {
        bail!("'{raw}' is not a registry URL: it must be absolute, carrying a scheme such as https://");
    };
    if scheme.is_empty() {
        bail!("'{raw}' is not a registry URL: it must be absolute, carrying a scheme such as https://");
    }
    let scheme = scheme.to_ascii_lowercase();

    // Split the authority from any path, then lowercase only the authority. A PATH IS CASE-SENSITIVE
    // and lowercasing the whole URL would silently rewrite a registry mounted under a mixed-case
    // prefix into one that answers 404.
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        bail!("'{raw}' is not a registry URL: it must carry a host after the scheme");
    }
    let authority = authority.to_ascii_lowercase();

    // A default port written out is the same registry as one left off, so it is dropped — otherwise
    // `https://r.example` and `https://r.example:443` hold two credentials for one service.
    let authority = match (scheme.as_str(), authority.rsplit_once(':')) {
        ("https", Some((host, "443"))) | ("http", Some((host, "80"))) => host.to_string(),
        _ => authority,
    };

    // No trailing slash — the v1 paths already begin with one, and `<base>//v1/...` is a different
    // path to most routers.
    let path = path.trim_end_matches('/');
    Ok(format!("{scheme}://{authority}{path}"))
}

/// A usable access token for a registry, refreshed if it is close to expiring.
pub async fn bearer_for(
    client: &reqwest::Client,
    session: &PersistedSession,
    registry: &TargetRegistry,
) -> Result<String> {
    // AN ENVIRONMENT TOKEN IS HONOURED ONLY FOR A REGISTRY NAMED IN THE SAME INVOCATION, and the
    // narrowing is the security property rather than a nicety.
    //
    // A developer who exports a token for registry A and later runs a command whose registry comes
    // from a persisted default would send A's bearer to B — a real credential leak to a third party,
    // caused by two facts that were each correct when they were set. Requiring the registry to be
    // named by `--registry` or by WAFFLER_REGISTRY in the same breath as the token means the pairing
    // is always something someone did on purpose.
    if let Some(token) = std::env::var(ENV_TOKEN).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        match registry.source {
            RegistrySource::Flag | RegistrySource::Environment => {
                println!("  using the bearer from {ENV_TOKEN} for {}", registry.base_url);
                return Ok(token);
            }
            RegistrySource::Default | RegistrySource::Builtin => {
                // SAID OUT LOUD RATHER THAN SILENTLY IGNORED. A token that is set and not used is
                // exactly the state where a developer concludes the tool is broken, and the fix is
                // one sentence naming what to add.
                eprintln!(
                    "  note: {ENV_TOKEN} is set but is NOT being used, because {} was not named in this \
                     invocation ({}). An environment token is only sent to a registry chosen by \
                     --registry or {ENV_REGISTRY}, so a token for one registry can never reach another.",
                    registry.base_url,
                    registry.source.because()
                );
            }
        }
    }

    let Some(credential) = session_store::credential_for(session, &registry.base_url) else {
        // NEVER AN EMPTY BEARER. Falling through to an unauthenticated request converts a local,
        // actionable refusal into a remote 401 that reads like a server problem.
        bail!(
            "not signed in to {}.\n  Run: waffler login --registry {}",
            registry.base_url,
            registry.base_url
        );
    };

    if credential.is_fresh(refresh_margin()) {
        return Ok(credential.access_token);
    }

    let Some(refresh_token) = credential.refresh_token.clone() else {
        bail!(
            "your session for {} has expired and the issuer granted no refresh token, so a browser \
             login is the only way forward.\n  Run: waffler login --registry {}",
            registry.base_url,
            registry.base_url
        );
    };

    match oidc_adapter::refresh(client, &credential.discovery_url, &credential.client_id, &refresh_token).await {
        Ok(tokens) => {
            let refreshed = credential_from_tokens(&registry.base_url, &credential.discovery_url, &credential.client_id, tokens, Some(&credential));
            session_store::put_credential(&refreshed)?;
            Ok(refreshed.access_token)
        }
        Err(e) => {
            // THE CREDENTIAL IS REMOVED. A stale entry that keeps failing looks to a developer like
            // the registry rejecting them; removing it makes the next message "you are not signed
            // in", which is both true and actionable.
            session_store::remove_credential(&registry.base_url)?;
            bail!(
                "your session for {} could not be refreshed and has been cleared ({e}).\n  Run: waffler login --registry {}",
                registry.base_url,
                registry.base_url
            );
        }
    }
}

/// Authenticate against whatever issuer a registry advertises, and persist the result for that
/// registry alone.
pub async fn sign_in(client: &reqwest::Client, registry: &TargetRegistry) -> Result<RegistryCredential> {
    let profile = registry_metadata_adapter::fetch_profile(client, &registry.base_url).await?;

    if !profile.authentication_available {
        bail!(
            "{} accepts no credentials, so there is nothing to sign in to. A read-only mirror is a \
             legitimate configuration; downloads from it need no session.",
            registry.base_url
        );
    }

    let discovery_url = profile
        .discovery_url
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
        // A REGISTRY THAT NAMES NO PROVIDER CANNOT BE SIGNED IN TO, and this says the REGISTRY did
        // not name one — not that the login failed. Substituting a default would authenticate the
        // developer against someone else's identity provider and send the token here, where it would
        // be refused for reasons naming neither.
        .with_context(|| {
            format!(
                "{} says authentication is available but does not name an OpenID discovery document, \
                 so this tool cannot know where to sign in.\n  A registry advertises \
                 `oidc_discovery_url` and `oidc_audience` on {}{}; until this deployment does, supply \
                 a bearer directly with {ENV_TOKEN} together with --registry.",
                registry.base_url,
                registry.base_url,
                registry_metadata_adapter::METADATA_PATH
            )
        })?
        .to_string();

    let audience = profile
        .audience
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .with_context(|| {
            format!(
                "{} names an issuer but no audience. A token minted for the wrong audience is refused \
                 by the registry, so guessing it would produce a login that appears to work and a \
                 publish that does not.",
                registry.base_url
            )
        })?
        .to_string();

    let client_id = profile
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or(DEFAULT_CLIENT_ID)
        .to_string();

    let tokens = oidc_adapter::authorize(client, &discovery_url, &audience, &client_id).await?;
    let credential = credential_from_tokens(&registry.base_url, &discovery_url, &client_id, tokens, None);
    session_store::put_credential(&credential)?;
    Ok(credential)
}

/// Forget the credential for one registry.
pub fn sign_out(registry: &TargetRegistry) -> Result<bool> {
    session_store::remove_credential(&registry.base_url)
}

/// Report the identity held for a registry, or that there is none.
pub fn who_am_i(session: &PersistedSession, registry: &TargetRegistry) -> Option<RegistryCredential> {
    session_store::credential_for(session, &registry.base_url)
}

/// Persist a registry as the default for later commands.
pub fn set_default(session: &PersistedSession, base_url: &str) -> Result<(TargetRegistry, bool)> {
    let normalised = normalise_registry_url(base_url)?;
    session_store::set_default_registry(&normalised)?;
    // Whether there is a credential is reported alongside, because otherwise a `use` to an
    // unauthenticated registry looks like a working session until the first publish.
    let signed_in = session_store::credential_for(session, &normalised).is_some();
    Ok((TargetRegistry { base_url: normalised, source: RegistrySource::Default }, signed_in))
}

/// What a registry says about itself.
pub async fn profile_of(client: &reqwest::Client, registry: &TargetRegistry) -> Result<RegistryProfile> {
    registry_metadata_adapter::fetch_profile(client, &registry.base_url).await
}

/// Turn an issuer's token set into a storable credential.
///
/// `previous` carries a rotated refresh token forward when the issuer sent none: AN ISSUER THAT
/// ROTATES AND A CLIENT THAT DROPS THE OLD TOKEN IS A SESSION THAT DIES ON THE SECOND REFRESH, and an
/// issuer that does NOT rotate sends nothing back — so the absence must mean "keep what you had",
/// not "you have none".
fn credential_from_tokens(
    registry: &str,
    discovery_url: &str,
    client_id: &str,
    tokens: super::types::TokenSet,
    previous: Option<&RegistryCredential>,
) -> RegistryCredential {
    // AN ABSOLUTE INSTANT, COMPUTED NOW. A duration persisted across a restart is a duration measured
    // from the wrong moment, and the error is always in the permissive direction: a token believed
    // fresh long after it expired.
    let lifetime = tokens.expires_in.unwrap_or(3600).max(0);
    let (subject, username) = claims_of(&tokens.access_token);
    RegistryCredential {
        registry: registry.to_string(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.or_else(|| previous.and_then(|p| p.refresh_token.clone())),
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(lifetime),
        subject,
        username,
        discovery_url: discovery_url.to_string(),
        client_id: client_id.to_string(),
    }
}

/// Read `sub` and `preferred_username` out of a JWT payload, FOR DISPLAY ONLY.
///
/// THE SIGNATURE IS NOT CHECKED AND MUST NOT BE. This is a client: the registry validates the token
/// against the issuer's key set and is the only party whose verdict matters. A client that verified
/// locally and refused to send a token it disliked would be enforcing its own reading of a policy it
/// does not own — and a client that read a SCOPE here and pre-judged a publish would be disagreeing
/// with the server about permission. Nothing decided anywhere in this tool depends on these two
/// strings; they exist so `whoami` can print a name.
fn claims_of(access_token: &str) -> (String, Option<String>) {
    let Some(payload) = access_token.split('.').nth(1) else {
        return (String::new(), None);
    };
    let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, payload) else {
        return (String::new(), None);
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return (String::new(), None);
    };
    (
        value.get("sub").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        value.get("preferred_username").and_then(|v| v.as_str()).map(str::to_string),
    )
}

#[cfg(test)]
#[path = "session_orchestrator.test.rs"]
mod tests;
