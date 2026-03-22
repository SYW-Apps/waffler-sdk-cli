use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const AUTH_SERVER: &str = "https://auth.sywapps.com";
/// Authentik application slug — must match the slug configured in the Authentik dashboard.
pub const APP_SLUG: &str = "waffler";
/// OAuth2 public client ID — must match the Client ID in the Authentik provider.
pub const CLIENT_ID: &str = "waffler";
pub const SCOPES: &str = "openid profile email";
/// Waffler developer registry API base URL.
pub const REGISTRY_API: &str = "https://registry.sywapps.com/v1/developer";

/// Fixed local port for the OAuth2 redirect. Register exactly this URI in Authentik:
///   http://127.0.0.1:17653/callback
const CALLBACK_PORT: u16 = 17653;

// ─── Stored credentials ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    /// Unix timestamp when the access_token expires.
    pub expires_at: Option<i64>,
    pub developer: DeveloperInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperInfo {
    pub username: String,
    pub email: String,
    /// Developer tags this account is authorized to publish under.
    /// Populated from the Waffler registry after login.
    /// Example: `["syw", "acme"]` means the developer can publish under `syw.*` and `acme.*`.
    #[serde(default)]
    pub namespaces: Vec<String>,
}

impl Credentials {
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            chrono::Utc::now().timestamp() >= exp - 30
        } else {
            false
        }
    }

    /// Returns true if this developer is allowed to publish under the given namespace.
    pub fn can_publish(&self, package_namespace: &str) -> bool {
        self.developer.namespaces.iter().any(|tag| {
            package_namespace == tag || package_namespace.starts_with(&format!("{}.", tag))
        })
    }
}

fn credentials_path() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("Cannot resolve home directory")?
        .join(".waffler");
    std::fs::create_dir_all(&dir).context("Cannot create ~/.waffler directory")?;
    Ok(dir.join("credentials.json"))
}

pub fn load_credentials() -> Option<Credentials> {
    let path = credentials_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_credentials(creds: &Credentials) -> Result<()> {
    let path = credentials_path()?;
    std::fs::write(path, serde_json::to_string_pretty(creds)?)
        .context("Cannot save credentials")?;
    Ok(())
}

pub fn clear_credentials() -> Result<()> {
    let path = credentials_path()?;
    if path.exists() {
        std::fs::remove_file(path).context("Cannot remove credentials file")?;
    }
    Ok(())
}

// ─── OIDC Discovery ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

async fn discover_endpoints(client: &reqwest::Client) -> Result<OidcDiscovery> {
    // No trailing slash — Authentik 2026.x returns 404 with trailing slash
    let url = format!(
        "{}/application/o/{}/.well-known/openid-configuration",
        AUTH_SERVER, APP_SLUG
    );

    let resp =
        client.get(&url).send().await.with_context(|| {
            format!("Cannot reach {AUTH_SERVER} — check your network connection")
        })?;

    if resp.status() == 404 {
        bail!(
            "OIDC discovery returned 404.\n\
             Check that the Authentik application slug is '{APP_SLUG}'.\n\
             Discovery URL tried: {url}"
        );
    }
    if !resp.status().is_success() {
        bail!("OIDC discovery returned {} for {url}", resp.status());
    }

    resp.json::<OidcDiscovery>()
        .await
        .context("Could not parse OIDC discovery document")
}

// ─── PKCE helpers ─────────────────────────────────────────────────────────────

fn generate_pkce() -> (String, String) {
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    let challenge_bytes = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(challenge_bytes);

    (code_verifier, code_challenge)
}

fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

// ─── Local redirect server ────────────────────────────────────────────────────

/// Starts a one-shot HTTP listener on CALLBACK_PORT.
/// Returns the `code` and `state` query parameters from the first request it receives.
async fn wait_for_callback() -> Result<(String, String)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(format!("127.0.0.1:{CALLBACK_PORT}"))
        .await
        .with_context(|| {
            format!(
                "Cannot bind to port {CALLBACK_PORT}. \
                 Is another process using it? Try closing it and retrying."
            )
        })?;

    let (mut stream, _) = listener
        .accept()
        .await
        .context("Failed to accept callback connection")?;

    let mut buf = vec![0u8; 8192];
    let n = stream
        .read(&mut buf)
        .await
        .context("Failed to read callback request")?;
    let request = std::str::from_utf8(&buf[..n]).unwrap_or("");

    // Parse the request line: GET /callback?code=xxx&state=yyy HTTP/1.1
    let (code, state) = parse_callback_params(request)?;

    // Send a friendly success page back to the browser
    let body = r#"<!DOCTYPE html>
<html>
<head><title>Waffler SDK — Login successful</title>
<style>body{font-family:sans-serif;text-align:center;padding:60px;background:#0f172a;color:#e2e8f0}
h1{color:#22c55e;font-size:2rem}p{color:#94a3b8}</style></head>
<body>
<h1>&#10003; Authenticated</h1>
<p>You can close this tab and return to the terminal.</p>
</body></html>"#;

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;

    Ok((code, state))
}

fn parse_callback_params(raw_request: &str) -> Result<(String, String)> {
    // First line: GET /callback?code=...&state=... HTTP/1.1
    let first_line = raw_request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");

    // Check for OAuth2 error
    let error: Option<&str> = query
        .split('&')
        .find(|p| p.starts_with("error="))
        .map(|p| &p["error=".len()..]);
    if let Some(err) = error {
        bail!("Authorization denied by auth server: {}", err);
    }

    let code = query
        .split('&')
        .find(|p| p.starts_with("code="))
        .map(|p| p["code=".len()..].to_string())
        .context("No authorization code in callback URL")?;

    let state = query
        .split('&')
        .find(|p| p.starts_with("state="))
        .map(|p| p["state=".len()..].to_string())
        .unwrap_or_default();

    Ok((code, state))
}

// ─── Token response ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct UserInfoResponse {
    preferred_username: Option<String>,
    email: Option<String>,
}

// ─── Registry API ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeveloperProfile {
    pub namespaces: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ClaimRequest<'a> {
    tag: &'a str,
}

#[derive(Debug, Deserialize)]
struct RegistryError {
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct NamespaceAvailability {
    pub available: bool,
    pub owner: Option<String>,
}

/// Fetch the developer's profile (including claimed namespaces) from the registry.
/// Returns `None` silently if the registry is unreachable or returns an error —
/// callers should fall back to empty namespaces and prompt the user to retry.
pub async fn fetch_profile(
    client: &reqwest::Client,
    access_token: &str,
) -> Option<DeveloperProfile> {
    client
        .get(format!("{}/profile", REGISTRY_API))
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?
        .json::<DeveloperProfile>()
        .await
        .ok()
}

/// Claim a new namespace tag for the authenticated developer.
pub async fn claim_namespace(
    client: &reqwest::Client,
    access_token: &str,
    tag: &str,
) -> Result<Vec<String>> {
    let resp = client
        .post(format!("{}/namespaces/claim", REGISTRY_API))
        .bearer_auth(access_token)
        .json(&ClaimRequest { tag })
        .send()
        .await
        .context("Registry request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err: RegistryError = resp.json().await.unwrap_or(RegistryError {
            message: format!("HTTP {status}"),
        });
        bail!("{}", err.message);
    }

    let profile: DeveloperProfile = resp
        .json()
        .await
        .context("Could not parse registry response")?;
    Ok(profile.namespaces)
}

/// Release (unclaim) a namespace tag.
pub async fn release_namespace(
    client: &reqwest::Client,
    access_token: &str,
    tag: &str,
) -> Result<Vec<String>> {
    let resp = client
        .delete(format!("{}/namespaces/{}", REGISTRY_API, tag))
        .bearer_auth(access_token)
        .send()
        .await
        .context("Registry request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err: RegistryError = resp.json().await.unwrap_or(RegistryError {
            message: format!("HTTP {status}"),
        });
        bail!("{}", err.message);
    }

    let profile: DeveloperProfile = resp
        .json()
        .await
        .context("Could not parse registry response")?;
    Ok(profile.namespaces)
}

/// Check whether a tag is available and who owns it if not.
pub async fn check_namespace(client: &reqwest::Client, tag: &str) -> Result<NamespaceAvailability> {
    let resp = client
        .get(format!("{}/namespaces/available", REGISTRY_API))
        .query(&[("tag", tag)])
        .send()
        .await
        .context("Registry request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        bail!("Registry returned {status}");
    }

    resp.json::<NamespaceAvailability>()
        .await
        .context("Could not parse availability response")
}

// ─── Public login entry point ─────────────────────────────────────────────────

/// Authorization Code + PKCE login flow.
///
/// 1. Discovers Authentik endpoints via OIDC well-known
/// 2. Generates PKCE verifier/challenge and CSRF state
/// 3. Opens the browser to the Authentik authorization page
/// 4. Waits for Authentik to redirect back to http://127.0.0.1:17653/callback
/// 5. Exchanges the authorization code for tokens
/// 6. Fetches userinfo and saves credentials
pub async fn login(client: &reqwest::Client) -> Result<Credentials> {
    let discovery = discover_endpoints(client).await?;

    let (code_verifier, code_challenge) = generate_pkce();
    let state = generate_state();
    let redirect_uri = format!("http://127.0.0.1:{CALLBACK_PORT}/callback");

    // Build the authorization URL
    let auth_url = {
        let mut url = reqwest::Url::parse(&discovery.authorization_endpoint)
            .context("Invalid authorization_endpoint in OIDC discovery")?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state);
        url.to_string()
    };

    println!("  Opening your browser...");
    println!("  If it doesn't open automatically, visit:");
    println!();
    println!("    {}", auth_url);
    println!();

    // Open browser (best-effort — non-fatal if it fails)
    if let Err(e) = open::that(&auth_url) {
        println!("  (Could not open browser automatically: {e})");
    }

    println!("  Waiting for authentication at http://127.0.0.1:{CALLBACK_PORT}/callback ...");

    // Wait for the redirect
    let (code, returned_state) = wait_for_callback().await?;

    // CSRF check
    if returned_state != state {
        bail!("State mismatch in OAuth2 callback — possible CSRF attack, aborting.");
    }

    // Exchange code for token
    let token_resp: TokenResponse = client
        .post(&discovery.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", code_verifier.as_str()),
        ])
        .send()
        .await
        .context("Token exchange request failed")?
        .json()
        .await
        .context("Could not parse token response")?;

    if let Some(err) = token_resp.error {
        bail!(
            "Token exchange failed: {} — {}",
            err,
            token_resp.error_description.as_deref().unwrap_or("")
        );
    }

    let access_token = token_resp
        .access_token
        .context("Token response is missing access_token")?;

    // Fetch userinfo
    let userinfo: UserInfoResponse = client
        .get(&discovery.userinfo_endpoint)
        .bearer_auth(&access_token)
        .send()
        .await
        .context("Userinfo request failed")?
        .json()
        .await
        .unwrap_or(UserInfoResponse {
            preferred_username: None,
            email: None,
        });

    let expires_at = token_resp
        .expires_in
        .map(|s| chrono::Utc::now().timestamp() + s as i64);

    // Fetch claimed namespaces from the developer registry.
    // Non-fatal: if the registry is unreachable the user can claim tags later.
    let namespaces = fetch_profile(client, &access_token)
        .await
        .map(|p| p.namespaces)
        .unwrap_or_default();

    if namespaces.is_empty() {
        println!(
            "  {} No developer tags claimed yet. Run `waffler namespace claim <tag>` to claim one.",
            console::style("hint:").dim()
        );
    }

    Ok(Credentials {
        access_token,
        refresh_token: token_resp.refresh_token,
        id_token: token_resp.id_token,
        expires_at,
        developer: DeveloperInfo {
            username: userinfo.preferred_username.unwrap_or_default(),
            email: userinfo.email.unwrap_or_default(),
            namespaces,
        },
    })
}
