//! `sdk_cli::publication-adapter` — every publishing request this tool makes, in one place.
//!
//! Spec: `sdk_cli::publication-adapter` / `ipublication-adapter` / `publication_adapter_impl`.
//!
//! ## WHY ONE COMPONENT
//!
//! The previous tool spread registry URLs across three files and got four routes wrong:
//! `POST /v1/packages` for a publish that is `POST /v1/packages/{namespace}/publish`;
//! `GET /v1/developer/profile` for an account read that is `GET /v1/developer/account`;
//! `POST /v1/developer/namespaces/claim` for a route that is `POST /v1/developer/namespaces`;
//! and a `GET /v1/developer/namespaces/available` that no longer exists at all.
//!
//! Four wrong calls in three files is what a scattered client looks like from the outside. One
//! component is what makes a route a fact a reader can check rather than a recollection.
//!
//! REFUSALS ARE REPORTED AS THE REGISTRY WORDED THEM. Its publication pipeline maps distinct refusals
//! to distinct statuses on purpose — not a publisher, no authority over this tag, a malformed
//! manifest, and a version already published are four different situations, and an automated
//! publisher told only "error" retries forever against a conflict.
//!
//! IT CARRIES A TOKEN AND JUDGES NOTHING. A 401 or 403 is surfaced, never pre-empted; a client that
//! decides in advance what a token may do is one that can disagree with the server about permission.

use std::path::Path;

use anyhow::{bail, Context, Result};

use super::types::{PublishOutcome, PublishedPackage, PublishedVersion};

/// The multipart field the registry names.
///
/// THE CANONICAL ONE. The registry also accepts `file` and `package`, and relying on that leniency is
/// relying on it continuing to exist.
const BUNDLE_FIELD: &str = "bundle";

/// `POST /v1/packages/{namespace}/publish`
pub async fn upload_bundle(
    client: &reqwest::Client,
    base_url: &str,
    namespace: &str,
    bundle_path: &Path,
    bearer: &str,
) -> Result<PublishOutcome> {
    let url = format!("{base_url}/v1/packages/{namespace}/publish");

    // STREAMED FROM THE FILE rather than read into memory. A bundle may be a hundred megabytes, and
    // the registry's size limit lives on the route as a body limit applied BEFORE the body is
    // buffered — a client that buffers first has already paid exactly what that limit exists to
    // avoid.
    let file = tokio::fs::File::open(bundle_path)
        .await
        .with_context(|| format!("reading {}", bundle_path.display()))?;
    let length = file
        .metadata()
        .await
        .with_context(|| format!("measuring {}", bundle_path.display()))?
        .len();
    let file_name = bundle_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{namespace}.zip"));

    let body = upload_body(file);
    let part = reqwest::multipart::Part::stream_with_length(body, length)
        .file_name(file_name)
        .mime_str("application/zip")
        .context("building the upload part")?;
    let form = reqwest::multipart::Form::new().part(BUNDLE_FIELD, part);

    let response = client
        .post(&url)
        .bearer_auth(bearer)
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        // THE REGISTRY'S OWN MESSAGE, CARRIED THROUGH. Flattening its distinct refusals into one
        // would discard the only part a developer can act on.
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_string))
            .unwrap_or_else(|| text.trim().to_string());
        bail!("the registry refused the publish ({status}): {message}");
    }

    let receipt: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let version = receipt
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    Ok(PublishOutcome {
        registry: base_url.to_string(),
        fqid: namespace.to_string(),
        version,
        bundle_path: bundle_path.to_path_buf(),
        receipt,
    })
}

/// `GET /v1/packages/{namespace}` — what the registry already holds.
///
/// THIS IS WHAT MAKES THE DOWNGRADE REFUSAL HONEST. Whether a package has ever carried a publisher
/// signature is the registry's fact; assuming it from local state would let a fresh clone downgrade a
/// package silently, which is the whole attack.
pub async fn fetch_published_package(
    client: &reqwest::Client,
    base_url: &str,
    namespace: &str,
) -> Result<Option<PublishedPackage>> {
    let url = format!("{base_url}/v1/packages/{namespace}");
    let response = client.get(&url).send().await.with_context(|| format!("could not reach {url}"))?;

    // A 404 IS AN ABSENCE, NOT AN ERROR. A first publish is the ordinary case, and turning it into a
    // failure would make the common path explain itself.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("{url} answered {status}: {}", text.trim());
    }

    let body: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("{url} did not answer with a package"))?;

    // The catalog's version list. Read defensively rather than through a mirrored struct: this tool
    // needs two fields out of it, and a full mirror of the catalog's shape would be a second
    // definition to keep in step for no gain.
    let versions = body
        .get("versions")
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| PublishedVersion {
                    version: entry.get("version").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    // ABSENT MEANS FALSE, and that is the fail-OPEN direction for a downgrade check —
                    // which is why it is worth naming. A registry that does not report publisher
                    // signatures at all cannot support the refusal, and pretending otherwise by
                    // defaulting to true would refuse every publish to such a registry instead. The
                    // honest position is that the check is only as strong as what the registry
                    // reports, and the orchestrator says so when it applies it.
                    publisher_signed: entry
                        .get("publisher_signed")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Some(PublishedPackage { versions }))
}

/// `DELETE /v1/packages/{namespace}/versions/{version}`
///
/// THE CATALOG ROW GOES AND THE ARTIFACT STAYS in custody, because another version may reference the
/// same bytes. That is the registry's decision, stated in the output so a developer is not told the
/// bytes were deleted when they were not.
pub async fn withdraw_version(
    client: &reqwest::Client,
    base_url: &str,
    namespace: &str,
    version: &str,
    bearer: &str,
) -> Result<()> {
    let url = format!("{base_url}/v1/packages/{namespace}/versions/{version}");
    let response = client
        .delete(&url)
        .bearer_auth(bearer)
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_else(|| text.trim().to_string());
    bail!("the registry refused the withdrawal ({status}): {message}");
}

/// Stream a file's bytes as an upload body.
///
/// `ReaderStream` reads in fixed chunks and yields them, so ONE CHUNK IS IN FLIGHT regardless of
/// artifact size. That is the whole reason the upload is a stream: a hundred-megabyte bundle read
/// whole into memory to be posted is exactly the cost the registry's before-buffering body limit
/// exists to avoid a client paying.
fn upload_body(file: tokio::fs::File) -> reqwest::Body {
    reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(file))
}
