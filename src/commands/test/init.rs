//! `waffler test init` — scaffold integration tests for all capabilities in the
//! current package directory.
//!
//! ## What it does
//!
//! 1. Reads `package.json` from the current (or specified) directory.
//! 2. Detects the runtime mode (`native`/`wasm`/`wasi`/`wasi_service`/`process`).
//! 3. Writes `tests/integration.rs` with one `#[tokio::test]` stub per declared
//!    capability, using the correct `PackageHarness::<runtime>(…).build()` constructor.
//! 4. Updates `Cargo.toml` `[dev-dependencies]` with `waffler-package-harness`
//!    if it is not already present (computes a relative path from the package dir
//!    to the harness crate inside the SDK).
//!
//! ## Flags
//!
//! - `--force` / `-f` : overwrite an existing `tests/integration.rs`.
//! - `--dir <path>`   : package directory (default: current directory).

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use std::path::{Path, PathBuf};

use crate::commands::{load_manifest, PackageManifest, RuntimeVariant};

// ── CLI args ──────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct InitArgs {
    /// Package directory (must contain package.json). Defaults to current directory.
    #[arg(short = 'd', long = "dir", default_value = ".")]
    pub dir: PathBuf,

    /// Overwrite tests/integration.rs if it already exists.
    #[arg(short = 'f', long = "force")]
    pub force: bool,
}

// ── Capability shape ──────────────────────────────────────────────────────────

/// Minimal fields from a capability entry in package.json.
#[derive(Debug, serde::Deserialize)]
struct CapabilityEntry {
    /// Stable identifier used as the call topic, e.g. `"audio.list_devices"`.
    id: String,
    /// Human-readable name, used as fallback for fn name derivation.
    #[serde(default)]
    name: Option<String>,
}

// ── Extended manifest (capabilities are not in the minimal PackageManifest) ───

#[derive(Debug, serde::Deserialize)]
struct FullManifest {
    #[serde(flatten)]
    base: PackageManifest,
    #[serde(default)]
    capabilities: Vec<CapabilityEntry>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(args: InitArgs) -> Result<()> {
    let pkg_dir = args
        .dir
        .canonicalize()
        .with_context(|| format!("Cannot resolve package directory: {:?}", args.dir))?;

    // ── Read manifest ──────────────────────────────────────────────────────────
    let manifest_path = pkg_dir.join("package.json");
    let manifest_raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("package.json not found in {:?}", pkg_dir))?;

    let manifest: FullManifest = serde_json::from_str(&manifest_raw)
        .with_context(|| format!("Invalid package.json at {:?}", manifest_path))?;

    let base = &manifest.base;
    let capabilities = &manifest.capabilities;

    // ── Detect runtime ─────────────────────────────────────────────────────────
    let runtime = base
        .get_runtime_variant()
        .context("Could not determine runtime variant from package.json")?;

    // ── Guard: no capabilities declared ───────────────────────────────────────
    if capabilities.is_empty() {
        println!(
            "{} No capabilities declared in package.json — nothing to scaffold.",
            style("hint:").dim()
        );
        println!(
            "  Add entries to the \"capabilities\" array in {:?} and re-run.",
            manifest_path
        );
        return Ok(());
    }

    // ── Guard: tests/integration.rs exists and --force not set ────────────────
    let tests_dir = pkg_dir.join("tests");
    let integration_path = tests_dir.join("integration.rs");

    if integration_path.exists() && !args.force {
        println!(
            "{} {} already exists.",
            style("warning:").yellow().bold(),
            integration_path.display()
        );
        println!("  Re-run with {} to overwrite.", style("--force").cyan());
        return Ok(());
    }

    // ── Locate the harness crate relative to pkg_dir ──────────────────────────
    // Expected layout: packages/<name>/  →  sdk/rust/waffler-package-harness/
    // We compute a relative path from pkg_dir.
    let harness_abs = find_harness_path(&pkg_dir)?;
    let harness_rel = make_relative(&harness_abs, &pkg_dir)
        .unwrap_or(harness_abs.clone());
    // Normalize separators to forward slashes for Cargo.toml portability.
    let harness_rel_str = harness_rel.to_string_lossy().replace('\\', "/");

    // ── Build the integration.rs source ───────────────────────────────────────
    let source = generate_integration_rs(
        &base.namespace,
        capabilities,
        runtime.as_ref(),
        &harness_rel_str,
    );

    // ── Write tests/integration.rs ────────────────────────────────────────────
    std::fs::create_dir_all(&tests_dir)
        .with_context(|| format!("Could not create tests/ directory at {:?}", tests_dir))?;

    std::fs::write(&integration_path, &source)
        .with_context(|| format!("Could not write {:?}", integration_path))?;

    println!(
        "{} Wrote {}",
        style("✓").green().bold(),
        style(integration_path.display()).underlined()
    );

    // ── Update Cargo.toml ──────────────────────────────────────────────────────
    let cargo_toml_path = pkg_dir.join("Cargo.toml");
    if cargo_toml_path.exists() {
        match maybe_add_harness_dev_dep(&cargo_toml_path, &harness_rel_str) {
            Ok(true) => {
                println!(
                    "{} Added waffler-package-harness to [dev-dependencies] in {}",
                    style("✓").green().bold(),
                    style(cargo_toml_path.display()).underlined()
                );
            }
            Ok(false) => {
                println!(
                    "  {} waffler-package-harness already in [dev-dependencies]",
                    style("hint:").dim()
                );
            }
            Err(e) => {
                println!(
                    "{} Could not update Cargo.toml: {}",
                    style("warning:").yellow().bold(),
                    e
                );
                println!(
                    "  Add manually: waffler-package-harness = {{ path = \"{}\" }}",
                    harness_rel_str
                );
            }
        }
    } else {
        println!(
            "  {} No Cargo.toml found — add dev-dependency manually:",
            style("hint:").dim()
        );
        println!(
            "  waffler-package-harness = {{ path = \"{}\" }}",
            harness_rel_str
        );
    }

    // ── Summary ────────────────────────────────────────────────────────────────
    println!();
    println!(
        "  Generated {} test stub(s) for {} (runtime: {}).",
        style(capabilities.len()).yellow(),
        style(&base.namespace).bold(),
        style(runtime_label(runtime.as_ref())).cyan()
    );
    println!();
    println!("  Next steps:");
    println!("    1. Fill in the TODO assertions in tests/integration.rs");
    println!(
        "    2. Set the artifact path environment variable or update the path constant"
    );
    println!("    3. cargo nextest run");

    Ok(())
}

// ── Code generation ───────────────────────────────────────────────────────────

fn generate_integration_rs(
    namespace: &str,
    capabilities: &[CapabilityEntry],
    runtime: Option<&RuntimeVariant>,
    harness_rel: &str,
) -> String {
    let constructor_hint = match runtime {
        Some(RuntimeVariant::Native) => {
            "PackageHarness::dll(DLL_PATH).namespace(NS).build().expect(\"build harness\")"
        }
        Some(RuntimeVariant::Wasm) => {
            "PackageHarness::wasm(WASM_PATH).namespace(NS).build().expect(\"build harness\")"
        }
        Some(RuntimeVariant::Wasi) => {
            "PackageHarness::wasi(WASM_PATH).namespace(NS).build().expect(\"build harness\")"
        }
        Some(RuntimeVariant::WasiService) => {
            "PackageHarness::wasi_service(WASM_PATH).namespace(NS).build().expect(\"build harness\")"
        }
        Some(RuntimeVariant::Process) => {
            "PackageHarness::external(BIN_PATH).namespace(NS).build().await.expect(\"build harness\")"
        }
        None => {
            "PackageHarness::dll(DLL_PATH).namespace(NS).build().expect(\"build harness\")"
        }
    };

    let (artifact_const, artifact_env) = artifact_constant(runtime);
    let is_async_build = matches!(runtime, Some(RuntimeVariant::Process));

    let stubs: Vec<String> = capabilities
        .iter()
        .map(|cap| generate_test_stub(cap, runtime, constructor_hint, is_async_build))
        .collect();

    format!(
        r#"//! Integration tests for {}. Generated by `waffler test init`.
//!
//! Run with:  cargo nextest run
//! Update snapshots:  UPDATE_SNAPSHOTS=1 cargo nextest run

use serde_json::json;
use waffler_package_harness::PackageHarness;

/// Namespace under test.
const NS: &str = "{}";

/// TODO: point this to your built artifact.
///
/// Recommended: set {} via your CI/CD environment or a .cargo/config.toml
/// `[env]` section. For local development you can hard-code the path.
{}

{}
"#,
        namespace,
        namespace,
        artifact_env,
        artifact_const,
        stubs.join("\n")
    )
}

/// Returns (const_declaration, env_var_name) for the artifact path constant.
fn artifact_constant(runtime: Option<&RuntimeVariant>) -> (String, &'static str) {
    match runtime {
        Some(RuntimeVariant::Wasm) | Some(RuntimeVariant::Wasi) | Some(RuntimeVariant::WasiService) => (
            r#"const WASM_PATH: &str = env!("CARGO_TARGET_TMPDIR"); // TODO: set WASM_PATH env or point here"#
                .to_string(),
            "WASM_PATH",
        ),
        Some(RuntimeVariant::Process) => (
            r#"const BIN_PATH: &str = env!("CARGO_TARGET_TMPDIR"); // TODO: set BIN_PATH env or point here"#
                .to_string(),
            "BIN_PATH",
        ),
        _ => (
            r#"const DLL_PATH: &str = env!("CARGO_TARGET_TMPDIR"); // TODO: set DLL_PATH env or point here"#
                .to_string(),
            "DLL_PATH",
        ),
    }
}

fn generate_test_stub(
    cap: &CapabilityEntry,
    runtime: Option<&RuntimeVariant>,
    constructor: &str,
    is_async_build: bool,
) -> String {
    let fn_name = capability_id_to_fn_name(&cap.id);
    let topic = &cap.id;
    let cap_display = cap
        .name
        .as_deref()
        .unwrap_or(topic.as_str());

    // For external-process runtime the builder is async.
    if is_async_build {
        return format!(
            r#"/// Test: {}
#[tokio::test]
async fn test_{}() {{
    let mut harness = {};

    harness.init().await.expect("init");

    let result = harness.call("{}", json!({{}})).await;
    // TODO: assert on result shape
    // Example: let data = result.expect("call succeeded");
    // assert!(data.is_object());
    assert!(result.is_ok(), "capability '{}' should succeed: {{:?}}", result.err());

    harness.assert_no_unhandled_panic();
}}
"#,
            cap_display,
            fn_name,
            constructor,
            topic,
            topic,
        );
    }

    format!(
        r#"/// Test: {}
#[tokio::test]
async fn test_{}() {{
    let mut harness = {};

    harness.init().await.expect("init");

    let result = harness.call("{}", json!({{}})).await;
    // TODO: assert on result shape
    // Example: let data = result.expect("call succeeded");
    // assert!(data.is_object());
    assert!(result.is_ok(), "capability '{}' should succeed: {{:?}}", result.err());

    harness.assert_no_unhandled_panic();
}}
"#,
        cap_display,
        fn_name,
        constructor,
        topic,
        topic,
    )
}

/// Convert a capability id like `"audio.list_devices"` to a valid Rust
/// snake_case function name: `"audio_list_devices"`.
fn capability_id_to_fn_name(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>()
        .to_lowercase()
}

fn runtime_label(runtime: Option<&RuntimeVariant>) -> &'static str {
    match runtime {
        Some(RuntimeVariant::Native) => "native (dll)",
        Some(RuntimeVariant::Wasm) => "wasm",
        Some(RuntimeVariant::Wasi) => "wasi",
        Some(RuntimeVariant::WasiService) => "wasi_service",
        Some(RuntimeVariant::Process) => "external process",
        None => "unknown",
    }
}

// ── Cargo.toml patching ───────────────────────────────────────────────────────

/// Add `waffler-package-harness` to `[dev-dependencies]` if not present.
///
/// Returns `Ok(true)` if the file was modified, `Ok(false)` if it already
/// contained the dep, `Err` on any I/O or parse failure.
fn maybe_add_harness_dev_dep(cargo_toml: &Path, harness_rel: &str) -> Result<bool> {
    let content = std::fs::read_to_string(cargo_toml)
        .context("read Cargo.toml")?;

    // If already present, nothing to do.
    if content.contains("waffler-package-harness") {
        return Ok(false);
    }

    let dep_line = format!(
        "\nwaffler-package-harness = {{ path = \"{}\" }}",
        harness_rel
    );

    let updated = if content.contains("[dev-dependencies]") {
        // Append the dep immediately after the `[dev-dependencies]` header.
        content.replacen("[dev-dependencies]", &format!("[dev-dependencies]{}", dep_line), 1)
    } else {
        // Append a new section at the end.
        format!("{}\n[dev-dependencies]{}\n", content.trim_end(), dep_line)
    };

    std::fs::write(cargo_toml, updated).context("write Cargo.toml")?;
    Ok(true)
}

// ── Harness path discovery ────────────────────────────────────────────────────

/// Compute a relative path from `from` directory to `to` path.
///
/// Returns `None` if the paths share no common prefix (e.g. different drive
/// letters on Windows), in which case the caller should fall back to the
/// absolute path.
fn make_relative(to: &Path, from: &Path) -> Option<PathBuf> {
    // Collect components of each.
    let mut from_iter = from.components().peekable();
    let mut to_iter = to.components().peekable();

    // Strip common prefix.
    while from_iter.peek().is_some() && from_iter.peek() == to_iter.peek() {
        from_iter.next();
        to_iter.next();
    }

    // For each remaining component in `from`, go up one level.
    let mut rel = PathBuf::new();
    for _ in from_iter {
        rel.push("..");
    }
    // Then descend into `to`.
    for comp in to_iter {
        rel.push(comp.as_os_str());
    }

    if rel.as_os_str().is_empty() {
        Some(PathBuf::from("."))
    } else {
        Some(rel)
    }
}

/// Find the absolute path to `sdk/rust/waffler-package-harness/` by walking
/// up from `pkg_dir` until we find the SDK root marker (`sdk/rust/waffler-package-harness/`).
///
/// Supports both:
///   - packages inside the monorepo: `<repo>/packages/<name>/`
///   - packages in external repos that use a path override via the
///     `WAFFLER_HARNESS_PATH` environment variable.
fn find_harness_path(pkg_dir: &Path) -> Result<PathBuf> {
    // Allow override via env var for external repos.
    if let Ok(override_path) = std::env::var("WAFFLER_HARNESS_PATH") {
        let p = PathBuf::from(override_path);
        if p.exists() {
            return p.canonicalize().context("WAFFLER_HARNESS_PATH does not exist");
        }
    }

    // Walk up the directory tree looking for sdk/rust/waffler-package-harness/.
    let mut current = pkg_dir.to_path_buf();
    loop {
        let candidate = current
            .join("sdk")
            .join("rust")
            .join("waffler-package-harness");
        if candidate.is_dir() {
            return candidate
                .canonicalize()
                .context("Could not canonicalize harness path");
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => bail!(
                "Could not locate sdk/rust/waffler-package-harness/ by walking up from {:?}.\n\
                 Hint: set WAFFLER_HARNESS_PATH to the absolute path of the harness crate.",
                pkg_dir
            ),
        }
    }
}
