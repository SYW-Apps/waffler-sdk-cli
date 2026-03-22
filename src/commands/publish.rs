use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use reqwest::multipart;
use std::path::PathBuf;

use crate::auth;
use crate::commands::{build, load_manifest};

const REGISTRY_PACKAGES_API: &str = "https://registry.sywapps.com/v1/packages";

#[derive(Args)]
pub struct PublishArgs {
    /// Package directory (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Path to an already-built ZIP to publish directly (skips build + pack)
    #[arg(long, short)]
    pub zip: Option<PathBuf>,

    /// Skip the build step and pack from existing artifacts
    #[arg(long)]
    pub no_build: bool,

    /// Registry base URL (overrides default)
    #[arg(long, env = "WAFFLER_REGISTRY")]
    pub registry: Option<String>,
}

pub async fn run(args: PublishArgs) -> Result<()> {
    let creds = auth::load_credentials()
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run `waffler login` first."))?;
    if creds.is_expired() {
        bail!("Auth token expired. Run `waffler login` to refresh.");
    }

    let zip_path = if let Some(zip) = args.zip {
        // Use the provided ZIP directly
        let zip = zip.canonicalize().context("ZIP file not found")?;
        println!(
            "{} Using pre-built ZIP: {}",
            style("→").cyan().bold(),
            style(zip.display()).white()
        );
        zip
    } else {
        // Build + pack first
        build_and_pack(&args.path, args.no_build, &creds).await?
    };

    let registry_base = args.registry.as_deref().unwrap_or(REGISTRY_PACKAGES_API);

    upload_zip(&zip_path, registry_base, &creds.access_token).await
}

async fn build_and_pack(
    pkg_dir_arg: &PathBuf,
    no_build: bool,
    creds: &auth::Credentials,
) -> Result<PathBuf> {
    let pkg_dir = pkg_dir_arg
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("Package directory not found: {:?}", pkg_dir_arg))?;

    let manifest = load_manifest(&pkg_dir)?;

    println!(
        "{} Publishing {} v{} ({}) ...",
        style("→").cyan().bold(),
        style(&manifest.display_name).bold(),
        manifest.version,
        manifest.namespace
    );

    // Ownership check
    if !creds.can_publish(&manifest.namespace) {
        let root_tag = manifest
            .namespace
            .split('.')
            .next()
            .unwrap_or(&manifest.namespace);
        bail!(
            "Namespace '{}' is not covered by any of your developer tags ({}).\n\
             Run `waffler namespace claim {}` to claim that tag first.",
            manifest.namespace,
            if creds.developer.namespaces.is_empty() {
                "none".to_string()
            } else {
                creds.developer.namespaces.join(", ")
            },
            root_tag
        );
    }

    // Delegate to pack logic — reuse pack::run internals via the PackArgs
    // Easiest: just call `waffler pack` as a subprocess so we don't duplicate the logic,
    // but since we're in the same binary we can call the pack function directly
    // with --output to a temp dir.
    let tmp_dir = std::env::temp_dir().join(format!(
        "waffler_publish_{}",
        manifest.namespace.replace('.', "_")
    ));
    std::fs::create_dir_all(&tmp_dir)?;

    let pack_args = crate::commands::pack::PackArgs {
        path: pkg_dir_arg.clone(),
        output: Some(tmp_dir.clone()),
        no_build,
        no_auth: false,
    };
    crate::commands::pack::run(pack_args).await?;

    // Find the ZIP that pack just produced
    let zip_name = format!(
        "waffler_pkg_{}_{}.zip",
        manifest.id.replace('.', "_"),
        manifest.version
    );
    let zip_path = tmp_dir.join(&zip_name);
    if !zip_path.exists() {
        bail!(
            "Pack step did not produce expected ZIP: {}",
            zip_path.display()
        );
    }

    Ok(zip_path)
}

async fn upload_zip(zip_path: &PathBuf, registry_base: &str, token: &str) -> Result<()> {
    let zip_bytes = std::fs::read(zip_path)
        .with_context(|| format!("Failed to read ZIP: {}", zip_path.display()))?;

    let zip_size_kb = zip_bytes.len() / 1024;

    let filename = zip_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "package.zip".to_string());

    println!(
        "  {} Uploading {} ({} KB)...",
        style("→").cyan(),
        style(&filename).white(),
        zip_size_kb
    );

    // Extract namespace from the filename to build the URL:
    // waffler_pkg_{namespace_underscores}_{version}.zip
    // We re-open it and read package.json instead — more reliable.
    let namespace = extract_namespace_from_zip(&zip_bytes)?;

    let url = format!("{}/{}/publish", registry_base, namespace);

    let part = multipart::Part::bytes(zip_bytes)
        .file_name(filename)
        .mime_str("application/zip")?;

    let form = multipart::Form::new().part("package", part);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .context("Failed to connect to registry")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status.is_success() {
        // Parse the response JSON for details
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            let ns = json
                .get("namespace")
                .and_then(|v| v.as_str())
                .unwrap_or(&namespace);
            let ver = json.get("version").and_then(|v| v.as_str()).unwrap_or("?");
            let vis = json
                .get("visibility")
                .and_then(|v| v.as_str())
                .unwrap_or("public");
            println!(
                "\n{} Published {}{} {} ({})",
                style("✓").green().bold(),
                style(ns).cyan().bold(),
                style(format!("@{ver}")).dim(),
                style(format!("[{vis}]")).yellow(),
                style("registry.waffler.sywapps.com").dim()
            );
        } else {
            println!("\n{} Published successfully.", style("✓").green().bold());
        }
    } else {
        // Try to extract a structured error message
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or(body);
        bail!("Registry returned {}: {}", status, message);
    }

    Ok(())
}

fn extract_namespace_from_zip(zip_bytes: &[u8]) -> Result<String> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("Invalid ZIP archive")?;
    let mut file = archive
        .by_name("package.json")
        .context("package.json not found in ZIP")?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let json: serde_json::Value = serde_json::from_str(&content).context("Invalid package.json")?;
    json.get("namespace")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("package.json missing 'namespace' field")
}
