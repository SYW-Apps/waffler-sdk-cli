//! `sdk_cli::oidc-adapter` — obtaining a token from whatever issuer a registry names.
//!
//! Spec: `sdk_cli::oidc-adapter` / `ioidc-adapter` / `oidc_adapter_impl`.
//!
//! WHAT SURVIVES FROM THE PREVIOUS TOOL, BECAUSE IT WAS RIGHT: an authorization-code flow with PKCE,
//! a loopback listener on an ephemeral port for the redirect, a browser handed the authorize URL, and
//! a refresh token exchanged rather than a re-prompt. That shape is correct and is kept. What was
//! wrong was the assumption around it — one issuer, one client id, both compiled in.
//!
//! TOKENS ARE RETURNED, NEVER STORED. Persistence belongs to the session store; a component that
//! both obtained and cached credentials is one where an "obtain" that quietly returned a cached value
//! would be indistinguishable from a real sign-in.
//!
//! NOTHING HERE VERIFIES A TOKEN. This is a client. The registry validates signatures against the
//! issuer's key set and is the only party whose verdict matters — a client enforcing its own reading
//! of a policy it does not own is a client that refuses requests the server would have accepted.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use base64::Engine;
use rand::Rng;
use sha2::Digest;

use super::types::TokenSet;

/// How long the loopback listener waits for the browser to come back.
///
/// A LOGIN THAT WAITS FOREVER IS ONE THAT HAS TO BE KILLED, and killing it leaves the terminal
/// without the sentence explaining what went wrong.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// The issuer's discovery document, reduced to what a flow needs.
#[derive(Debug, Clone, serde::Deserialize)]
struct Discovery {
    #[serde(default)]
    authorization_endpoint: Option<String>,
    #[serde(default)]
    token_endpoint: Option<String>,
}

/// Fetch and reduce an issuer's discovery document.
///
/// A DOCUMENT THAT NAMES NEITHER ENDPOINT IS A REFUSAL THAT SAYS SO. The alternative — guessing
/// conventional paths — produces requests to endpoints that may exist and may belong to something
/// else entirely.
async fn discover(client: &reqwest::Client, issuer: &str) -> Result<Discovery> {
    let issuer = issuer.trim_end_matches('/');
    // Two spellings, because a minimal provider may serve the document at the bare path while the
    // specification places it under `/.well-known`. Tried in specification order.
    let candidates = [
        format!("{issuer}/.well-known/openid-configuration"),
        format!("{issuer}/openid-configuration"),
    ];
    let mut last = String::new();
    for url in &candidates {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                let body = response.text().await.unwrap_or_default();
                if let Ok(d) = serde_json::from_str::<Discovery>(&body) {
                    return Ok(d);
                }
                last = format!("{url} did not answer with a discovery document");
            }
            Ok(response) => last = format!("{url} answered {}", response.status()),
            Err(e) => last = format!("{url} could not be reached: {e}"),
        }
    }
    bail!("could not read the OpenID discovery document from issuer {issuer} ({last})");
}

/// Run an authorization-code flow with PKCE against a discovered issuer.
pub async fn authorize(
    client: &reqwest::Client,
    issuer: &str,
    audience: &str,
    client_id: &str,
) -> Result<TokenSet> {
    let discovery = discover(client, issuer).await?;
    let authorization_endpoint = discovery
        .authorization_endpoint
        .clone()
        .with_context(|| format!("issuer {issuer} names no authorization endpoint, so an interactive login cannot be started"))?;
    let token_endpoint = discovery
        .token_endpoint
        .clone()
        .with_context(|| format!("issuer {issuer} names no token endpoint, so a code cannot be exchanged"))?;

    // PKCE IS NOT OPTIONAL HERE, AND THE LOOPBACK REDIRECT IS WHY.
    //
    // A redirect to `http://127.0.0.1:<port>` is reachable by any other process on the machine, so an
    // authorization code delivered there is a code anybody local could race for. The verifier never
    // leaves this process, which turns a stolen code into nothing. A public client on a loopback
    // redirect WITHOUT PKCE is the textbook shape of this vulnerability, not a hardening opportunity.
    let verifier = random_urlsafe(64);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe(32);

    // THE PORT IS EPHEMERAL, NOT FIXED. A fixed port is one another process can already be holding,
    // and the failure is a login that hangs rather than one that says the port is taken.
    let listener = TcpListener::bind("127.0.0.1:0").context("could not open a loopback listener for the login redirect")?;
    let port = listener.local_addr().context("reading the listener address")?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let authorize_url = {
        let mut url = format!("{authorization_endpoint}?response_type=code");
        for (k, v) in [
            ("client_id", client_id),
            ("redirect_uri", redirect_uri.as_str()),
            ("scope", "openid profile email offline_access"),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("audience", audience),
        ] {
            url.push('&');
            url.push_str(&format!("{k}={}", urlencode(v)));
        }
        url
    };

    // PRINTED AS WELL AS OPENED. A headless or remote shell has no browser to open, and a URL on
    // stdout is the only way through — a tool that only opens a browser is a tool that cannot be
    // used over ssh.
    println!("Opening your browser to sign in. If it does not open, visit:\n  {authorize_url}\n");
    let _ = open::that(&authorize_url);

    let code = wait_for_code(listener, &state)?;

    let response = client
        .post(&token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client_id),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .with_context(|| format!("could not reach the token endpoint {token_endpoint}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("the issuer refused the authorization code ({status}): {}", body.trim());
    }
    serde_json::from_str(&body).with_context(|| format!("the token endpoint {token_endpoint} did not answer with a token set"))
}

/// Exchange a refresh token for a new access token.
///
/// A FAILED REFRESH IS REPORTED AS FAILED, not as a silent re-prompt. The caller decides whether to
/// discard the credential and ask for a browser — an adapter that transparently escalated to an
/// interactive flow would open a browser from inside a CI run and hang it until the timeout.
pub async fn refresh(
    client: &reqwest::Client,
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenSet> {
    let discovery = discover(client, issuer).await?;
    let token_endpoint = discovery
        .token_endpoint
        .with_context(|| format!("issuer {issuer} names no token endpoint, so a refresh cannot be exchanged"))?;

    let response = client
        .post(&token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ])
        .send()
        .await
        .with_context(|| format!("could not reach the token endpoint {token_endpoint}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("the issuer refused the refresh token ({status}): {}", body.trim());
    }
    serde_json::from_str(&body).context("the token endpoint did not answer with a token set")
}

/// Accept exactly one request on the loopback listener and read the code out of it.
fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    listener
        .set_nonblocking(false)
        .context("configuring the loopback listener")?;

    let deadline = std::time::Instant::now() + CALLBACK_TIMEOUT;
    // A read timeout per connection, so a connection that opens and says nothing cannot hold the
    // login open past the deadline.
    for stream in listener.incoming() {
        if std::time::Instant::now() > deadline {
            bail!("the browser did not come back within {} seconds", CALLBACK_TIMEOUT.as_secs());
        }
        let mut stream = stream.context("accepting the login redirect")?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();

        let mut request_line = String::new();
        BufReader::new(&stream).read_line(&mut request_line).context("reading the login redirect")?;

        // "GET /callback?code=...&state=... HTTP/1.1"
        let target = request_line.split_whitespace().nth(1).unwrap_or_default().to_string();
        let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
        let mut code = None;
        let mut state = None;
        let mut error = None;
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("code", v)) => code = Some(urldecode(v)),
                Some(("state", v)) => state = Some(urldecode(v)),
                Some(("error", v)) => error = Some(urldecode(v)),
                _ => {}
            }
        }

        let (page, result) = match (error, code, state) {
            (Some(e), _, _) => (
                format!("Sign-in failed: {e}"),
                Err(anyhow::anyhow!("the issuer reported '{e}'")),
            ),
            // THE STATE PARAMETER IS CHECKED AND A MISMATCH ABORTS.
            //
            // The listener accepts one request from anywhere on the loopback interface; without this
            // check another local process can complete the flow with a code for a DIFFERENT account,
            // and this tool would cache it as the developer's and publish under it.
            (_, _, s) if s.as_deref() != Some(expected_state) => (
                "Sign-in failed: the response did not match this request.".to_string(),
                Err(anyhow::anyhow!(
                    "the login redirect carried a state value this process did not issue; refusing it, \
                     because accepting one would mean caching a credential for whoever did issue it"
                )),
            ),
            (_, Some(c), _) => (
                "Signed in. You can close this tab and return to the terminal.".to_string(),
                Ok(c),
            ),
            (_, None, _) => (
                "Sign-in failed: no authorization code was returned.".to_string(),
                Err(anyhow::anyhow!("the login redirect carried no authorization code")),
            ),
        };

        // Answered before the result is returned either way, so the developer's browser never hangs
        // on a request this process abandoned.
        let body = format!("<!doctype html><meta charset=utf-8><title>Waffler</title><p>{page}");
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.flush();

        return result;
    }
    bail!("the loopback listener closed before the browser came back")
}

/// A URL-safe random string with no padding.
fn random_urlsafe(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill(&mut buffer[..]);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer)
}

/// Percent-encode everything that is not unreserved.
///
/// WRITTEN OUT RATHER THAN PULLED IN. The set is three lines and one dependency fewer; more to the
/// point, a query value that is under-encoded in an authorize URL is a parameter-injection bug, so
/// the rule should be visible where it is used rather than a function name to trust.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Reverse of [`urlencode`], plus the `+`-means-space convention a form-encoded query uses.
fn urldecode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&value[i + 1..i + 3], 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
#[path = "oidc_adapter.test.rs"]
mod tests;
