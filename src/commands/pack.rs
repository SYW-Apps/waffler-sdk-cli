use anyhow::{bail, Result};
use clap::Args;
use console::style;
use std::path::PathBuf;

use crate::auth;
use crate::commands::{build, load_manifest};
use crate::segment;
use crate::zip_builder::ZipBuilder;

#[derive(Args)]
pub struct PackArgs {
    /// Package directory (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Output path for the generated ZIP (defaults to current directory)
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Skip the build step (use existing artifacts)
    #[arg(long)]
    pub no_build: bool,

    /// Allow packing without a logged-in developer account (skips namespace check)
    #[arg(long)]
    pub no_auth: bool,
}

pub async fn run(args: PackArgs) -> Result<()> {
    let pkg_dir = args.path.canonicalize()
        .map_err(|_| anyhow::anyhow!("Package directory not found: {:?}", args.path))?;

    let manifest = load_manifest(&pkg_dir)?;

    println!(
        "{} Packing {} v{} ({}) ...",
        style("→").cyan().bold(),
        style(&manifest.display_name).bold(),
        manifest.version,
        manifest.namespace
    );

    // ─── Namespace ownership check ────────────────────────────────────────────
    if !args.no_auth {
        match auth::load_credentials() {
            None => {
                bail!(
                    "Not logged in. Run `waffler login` first, or use --no-auth to skip ownership validation."
                );
            }
            Some(creds) => {
                if creds.is_expired() {
                    bail!("Auth token expired. Run `waffler login` to refresh.");
                }
                if !creds.can_publish(&manifest.namespace) {
                    let root_tag = manifest.namespace.split('.').next().unwrap_or(&manifest.namespace);
                    let owned = if creds.developer.namespaces.is_empty() {
                        "none".to_string()
                    } else {
                        creds.developer.namespaces.join(", ")
                    };
                    bail!(
                        "Namespace '{}' is not covered by any of your developer tags ({}).\n\
                         Run `waffler namespace claim {}` to claim that tag first.",
                        manifest.namespace,
                        owned,
                        root_tag
                    );
                }
                println!(
                    "  {} Namespace ownership verified ({})",
                    style("✓").green(),
                    style(&creds.developer.username).cyan()
                );
            }
        }
    }

    let language = manifest.build.as_ref().map(|b| b.language.as_str()).unwrap_or("rust");

    // ─── Build ────────────────────────────────────────────────────────────────
    if !args.no_build && language != "waffler_native" {
        println!("  {} Building ({})...", style("→").cyan(), language);
        match language {
            "rust" => build::build_rust(
                &pkg_dir,
                &manifest.id,
                manifest.features.native_module,
                true, // always release for packing
            )?,
            "node" => build::build_node(&pkg_dir)?,
            "python" => build::build_python(&pkg_dir)?,
            other => bail!("Unsupported build language: '{}'", other),
        }
        println!("  {} Build complete.", style("✓").green());
    }

    // ─── Locate artifacts ─────────────────────────────────────────────────────
    let artifact_path: Option<PathBuf> = if manifest.features.native_module {
        let module_path = manifest
            .module
            .as_ref()
            .map(|m| m.module_path.as_str())
            .unwrap_or_default();

        // WASM artifacts land in target/wasm32-unknown-unknown/release/ relative to workspace root
        // Try multiple candidate locations
        let candidates = [
            pkg_dir.join(module_path),
            find_workspace_root(&pkg_dir)
                .map(|r| r.join("target/wasm32-unknown-unknown/release").join(module_path))
                .unwrap_or_default(),
        ];
        candidates.into_iter().find(|p| !p.as_os_str().is_empty() && p.exists())
    } else if manifest.features.service {
        let exe_name = manifest
            .process
            .as_ref()
            .and_then(|p| p.entrypoint.as_deref())
            .unwrap_or(&manifest.id);
        let exe_ext = if cfg!(windows) { ".exe" } else { "" };
        let exe_file = format!("{}{}", exe_name, exe_ext);
        let candidates = [
            pkg_dir.join(&exe_file),
            find_workspace_root(&pkg_dir)
                .map(|r| r.join("target/release").join(&exe_file))
                .unwrap_or_default(),
        ];
        candidates.into_iter().find(|p| !p.as_os_str().is_empty() && p.exists())
    } else {
        None
    };

    if (manifest.features.native_module || manifest.features.service) && artifact_path.is_none() {
        bail!(
            "Built artifact not found. Run `waffler build` first or check your build.language setting."
        );
    }

    // ─── Process namespace/ tree ──────────────────────────────────────────────
    let namespace_src = pkg_dir.join("namespace");
    if namespace_src.exists() {
        // Copy to a temp staging dir and generate segment.json files there
        // so we don't pollute the source tree
        let staging_ns = std::env::temp_dir().join(format!(
            "waffler_pack_{}_ns",
            manifest.id.replace('.', "_")
        ));
        if staging_ns.exists() {
            std::fs::remove_dir_all(&staging_ns)?;
        }
        copy_dir(&namespace_src, &staging_ns)?;

        let generated = segment::generate_segment_manifests(
            &staging_ns,
            &manifest.namespace,
            &manifest.display_name,
        )?;
        println!(
            "  {} Generated {} segment.json file{}",
            style("✓").green(),
            generated.len(),
            if generated.len() == 1 { "" } else { "s" }
        );

        // Build the ZIP
        let zip_name = format!(
            "waffler_pkg_{}_{}.zip",
            manifest.id.replace('.', "_"),
            manifest.version
        );
        let zip_out = args
            .output
            .as_deref()
            .map(|p| p.join(&zip_name))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join(&zip_name));

        let mut builder = ZipBuilder::new();

        // package.json at root
        builder.add_file(&pkg_dir.join("package.json"), "package.json");

        // Artifact at root
        if let Some(ref artifact) = artifact_path {
            let fname = artifact.file_name().unwrap_or_default().to_string_lossy();
            builder.add_file(artifact, &fname);
        }

        // www/ (built UI assets)
        for ui_dir in ["webapp/dist", "webapp/build", "ui/dist", "ui/build", "web/dist"] {
            let dist = pkg_dir.join(ui_dir);
            if dist.exists() {
                builder.add_dir(&dist, "www")?;
                break;
            }
        }

        // namespace/ tree (from staging copy with generated segment.json)
        builder.add_dir(&staging_ns, "namespace")?;

        builder.write(&zip_out)?;

        // Clean up staging
        let _ = std::fs::remove_dir_all(&staging_ns);

        println!(
            "\n{} Package created: {}",
            style("✓").green().bold(),
            style(zip_out.display()).white().bold()
        );
    } else {
        // No namespace/ — still build a ZIP with just package.json + artifact
        let zip_name = format!(
            "waffler_pkg_{}_{}.zip",
            manifest.id.replace('.', "_"),
            manifest.version
        );
        let zip_out = args
            .output
            .as_deref()
            .map(|p| p.join(&zip_name))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join(&zip_name));

        let mut builder = ZipBuilder::new();
        builder.add_file(&pkg_dir.join("package.json"), "package.json");
        if let Some(ref artifact) = artifact_path {
            let fname = artifact.file_name().unwrap_or_default().to_string_lossy();
            builder.add_file(artifact, &fname);
        }
        builder.write(&zip_out)?;

        println!(
            "\n{} Package created (no namespace/): {}",
            style("⚠").yellow().bold(),
            style(zip_out.display()).white().bold()
        );
        println!(
            "  {} Consider adding a `namespace/` directory for entity auto-discovery.",
            style("hint:").dim()
        );
    }

    Ok(())
}

fn find_workspace_root(start: &std::path::Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("Cargo.toml").exists() {
            let content = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
            if content.contains("[workspace]") {
                return Some(dir);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let rel = entry.path().strip_prefix(src).unwrap_or(entry.path());
        let dst_path = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dst_path)?;
        } else {
            if let Some(p) = dst_path.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}
