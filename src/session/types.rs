//! The session subsystem's data model.
//!
//! Spec: `sdk_cli::session` types — `target-registry`, `registry-credential`, `registry-profile`.

use serde::{Deserialize, Serialize};

/// Which of the four sources decided the registry.
///
/// CARRIED BECAUSE THE ANSWER ALONE IS NOT ENOUGH. Four resolution sources means four ways to be
/// surprised, and "publishing to https://registry.waffler.dev" answers a different question from
/// "publishing to https://registry.waffler.dev, because WAFFLER_REGISTRY is set in this shell". The
/// second one is what lets a developer notice, before the upload, that they are not where they
/// thought.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrySource {
    /// `--registry` on the command. The most explicit wins.
    Flag,
    /// `WAFFLER_REGISTRY` in the environment.
    Environment,
    /// The default persisted by `waffler use`.
    Default,
    /// The compiled-in public registry. LAST, not first — a compiled-in registry as the primary
    /// answer is exactly the assumption this subsystem exists to remove.
    Builtin,
}

impl RegistrySource {
    /// How the source reads in a one-line report.
    pub fn because(self) -> &'static str {
        match self {
            Self::Flag => "--registry was passed",
            Self::Environment => "WAFFLER_REGISTRY is set in this environment",
            Self::Default => "it is the default set by `waffler use`",
            Self::Builtin => "no registry was chosen, so the built-in default applies",
        }
    }
}

/// Which registry this invocation addresses, and why.
#[derive(Debug, Clone)]
pub struct TargetRegistry {
    /// The NORMALISED base URL, with no trailing slash — the v1 paths already begin with one. This
    /// exact string is the credential store's key.
    pub base_url: String,
    pub source: RegistrySource,
}

/// One signed-in session, belonging to exactly one registry.
///
/// KEYED BY REGISTRY, WHICH IS A TRUST BOUNDARY RATHER THAN A CONVENIENCE. Two registries are two
/// services with two identity providers and two ideas of who owns which namespace. One cached token
/// shared between them would let a `use` change carry an identity across that boundary silently, and
/// the request that arrived at the wrong registry would carry a perfectly valid bearer for somewhere
/// else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryCredential {
    /// The normalised registry base URL this credential is for.
    pub registry: String,
    pub access_token: String,
    /// Absent when the issuer grants none, in which case expiry means a real re-login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// When the access token stops being accepted. Compared against a margin so a token cannot
    /// expire between the check and the upload it was checked for.
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// The `sub` claim, for display. Stable across username changes, which is why it and not the
    /// username is shown when they disagree.
    #[serde(default)]
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// The discovery document of the provider that minted it, so a refresh knows where to go.
    ///
    /// STORED RATHER THAN RE-DISCOVERED: a refresh must reach the provider that minted the token, and
    /// asking the registry again would let a re-configured deployment send this tool's refresh
    /// somewhere other than where the grant is held.
    #[serde(default, alias = "issuer")]
    pub discovery_url: String,
    /// The public client id used, for the same reason as the issuer.
    #[serde(default)]
    pub client_id: String,
}

impl RegistryCredential {
    /// Is the access token far enough from expiry to survive the work it is about to be used for?
    ///
    /// THE MARGIN IS THE POINT. A token that expires between the check and a hundred-megabyte upload
    /// wastes the upload; refreshing on a margin costs one request.
    pub fn is_fresh(&self, margin: chrono::Duration) -> bool {
        self.expires_at > chrono::Utc::now() + margin
    }
}

/// What a registry says about itself.
///
/// AN ABSENT ISSUER IS REPORTED AS MISSING, NEVER SUBSTITUTED. Falling back to a default issuer
/// would authenticate a developer against someone else's identity provider and send the resulting
/// token to this one, where it would be refused for reasons that name neither.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProfile {
    /// Whether the instance accepts credentials at all. False means every write route refuses
    /// regardless of what is presented.
    #[serde(default)]
    pub authentication_available: bool,
    /// The OpenID discovery document this instance trusts.
    ///
    /// FETCHED DIRECTLY rather than derived from an issuer, so no conventional path is ever guessed
    /// at. Absent means the instance did not say, which is a refusal to log in rather than a licence
    /// to guess: substituting a default would authenticate a developer against somebody else's
    /// identity provider and present the resulting token here, to be refused for reasons naming
    /// neither party.
    #[serde(default, rename = "oidc_discovery_url")]
    pub discovery_url: Option<String>,
    /// The audience this instance requires in a token. A token minted for a different audience is
    /// refused by the registry, so getting it from the registry is the only way to be right.
    #[serde(default, rename = "oidc_audience")]
    pub audience: Option<String>,
    /// The public OAuth client this tool should identify as.
    ///
    /// OPTIONAL WHERE THE OTHER TWO ARE NOT: a client id is a public identifier the provider either
    /// recognises or rejects, so falling back to a default produces a clean refusal from the right
    /// party.
    #[serde(default, rename = "oidc_client_id")]
    pub client_id: Option<String>,
    /// The largest upload this instance accepts.
    #[serde(default)]
    pub max_package_size_bytes: u64,
    /// Whether this instance can sign and accept publishes at all. A mirror that only serves
    /// downloads answers false, and telling a developer that before a build is worth the request.
    #[serde(default)]
    pub publishing_available: bool,
    /// The keys this registry signs bundles with, as it reports them.
    ///
    /// Carried so a publish can state which identity will vouch for the artifact. THIS TOOL VERIFIES
    /// NOTHING — the node that installs the package holds the trust anchors and is the only party
    /// whose verdict matters.
    #[serde(default)]
    pub signing_identities: Vec<serde_json::Value>,
    /// Whether downloads need no credential. Reported for completeness; nothing here decides on it.
    #[serde(default)]
    pub public_downloads: bool,
}

/// The persisted session document: a default registry, and one credential per registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_registry: Option<String>,
    /// Keyed by normalised registry base URL. A map rather than a list, because the key IS the
    /// invariant: one credential per registry, enforced by the shape rather than by a check.
    #[serde(default)]
    pub credentials: std::collections::BTreeMap<String, RegistryCredential>,
}

/// What an issuer returned from a token endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Seconds. Turned into an absolute instant before it is stored, because a duration persisted
    /// across a restart is a duration measured from the wrong moment.
    #[serde(default)]
    pub expires_in: Option<i64>,
}
