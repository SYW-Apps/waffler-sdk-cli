use anyhow::{bail, Result};
use clap::Args;
use console::style;
use std::path::PathBuf;

use crate::auth;
use crate::commands::load_manifest;

#[derive(Args)]
pub struct ValidateArgs {
    /// Package directory (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Validate namespace ownership against logged-in account
    #[arg(long)]
    pub check_ownership: bool,
}

pub async fn run(args: ValidateArgs) -> Result<()> {
    let mut errors: Vec<String> = vec![];
    let mut warnings: Vec<String> = vec![];

    println!(
        "{} Validating package at {:?} ...",
        style("→").cyan().bold(),
        args.path
    );

    // 1. Parse manifest
    let manifest = match load_manifest(&args.path) {
        Ok(m) => m,
        Err(e) => bail!("Cannot load manifest: {}", e),
    };

    println!(
        "  Package: {} ({})",
        style(&manifest.display_name).bold(),
        manifest.namespace
    );

    // 2. Required fields
    if manifest.namespace.is_empty() {
        errors.push("manifest: `namespace` is empty".into());
    }
    if manifest.id.is_empty() {
        errors.push("manifest: `id` is empty".into());
    }
    if manifest.version.is_empty() {
        errors.push("manifest: `version` is empty".into());
    }

    // 3. Namespace format (dot-separated, no spaces, lowercase preferred)
    let ns = &manifest.namespace;
    if ns.contains(' ') {
        errors.push(format!("manifest: namespace '{}' contains spaces", ns));
    }
    if ns.starts_with('.') || ns.ends_with('.') {
        errors.push(format!(
            "manifest: namespace '{}' has leading/trailing dot",
            ns
        ));
    }

    // 4. Feature + config consistency
    if manifest.features.native_module && manifest.module.is_none() {
        errors.push(
            "manifest: `features.native_module` is true but `module` config is missing".into(),
        );
    }
    if manifest.features.service && manifest.process.is_none() {
        errors.push("manifest: `features.service` is true but `process` config is missing".into());
    }
    if manifest.build.is_none() {
        warnings.push(
            "manifest: `build.language` is not set — consider adding it for waffler-cli tooling"
                .into(),
        );
    }

    // 5. namespace/ directory
    let namespace_dir = args.path.join("namespace");
    if !namespace_dir.exists() {
        warnings.push(
            "No `namespace/` directory found. Entities will not be auto-discovered on install."
                .into(),
        );
    } else {
        // Check namespace path matches manifest
        let ns_path: PathBuf = manifest.namespace.split('.').collect();
        let expected_ns_dir = namespace_dir.join(&ns_path);
        if !expected_ns_dir.exists() {
            errors.push(format!(
                "namespace/: Expected directory `namespace/{}/` for namespace `{}` not found",
                ns_path.display(),
                manifest.namespace
            ));
        }

        // Check for unexpected file types in namespace/
        for entry in walkdir::WalkDir::new(&namespace_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let ext = entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if ext != "json" {
                warnings.push(format!(
                    "namespace/: Non-JSON file will be excluded from package: {:?}",
                    entry
                        .path()
                        .strip_prefix(&args.path)
                        .unwrap_or(entry.path())
                ));
            }
        }
    }

    // 6. Permission groups: validate structure
    {
        let groups = &manifest.permissions.groups;
        let mut seen_ids = std::collections::HashSet::new();
        for group in groups {
            if group.id.is_empty() {
                errors.push("permissions.groups: a group has an empty `id`".into());
            } else if !seen_ids.insert(group.id.clone()) {
                errors.push(format!("permissions.groups: duplicate group id '{}'", group.id));
            }
            if group.label.is_empty() {
                errors.push(format!("permissions.groups[{}]: `label` is empty", group.id));
            }
            for (i, rule) in group.rules.iter().enumerate() {
                if rule.pattern.path.is_empty() {
                    errors.push(format!(
                        "permissions.groups[{}].rules[{}]: `pattern.path` is empty",
                        group.id, i
                    ));
                }
            }
            if group.rules.is_empty() {
                warnings.push(format!(
                    "permissions.groups[{}]: no rules defined — group will have no effect",
                    group.id
                ));
            }
        }
        if groups.is_empty() {
            warnings.push(
                "permissions.groups is empty — package will have no bus permissions. Add groups if your package calls other services.".into()
            );
        }
    }

    // 7. Native module: .wasm file (only relevant if already built)
    if manifest.features.native_module {
        if let Some(module) = &manifest.module {
            let wasm_path = args.path.join(&module.module_path);
            if !wasm_path.exists() {
                warnings.push(format!(
                    "WASM module `{}` not found — run `waffler build` first",
                    module.module_path
                ));
            }
        }
    }

    // 7. Namespace ownership check
    if args.check_ownership {
        let client = reqwest::Client::new();
        match auth::load_and_refresh(&client).await {
            Err(_) => {
                warnings.push(
                    "Not logged in or session could not be refreshed — cannot verify namespace ownership. Run `waffler login`."
                        .into(),
                );
            }
            Ok(creds) => {
                if !creds.can_publish(&manifest.namespace) {
                    let owned = if creds.developer.namespaces.is_empty() {
                        "none".to_string()
                    } else {
                        creds.developer.namespaces.join(", ")
                    };
                    errors.push(format!(
                        "Namespace ownership: '{}' is not covered by any of your developer tags ({})",
                        manifest.namespace,
                        owned
                    ));
                } else {
                    println!(
                        "  {} Namespace ownership verified for {}",
                        style("✓").green(),
                        style(&creds.developer.username).cyan()
                    );
                }
            }
        }
    }

    // ─── Summary ─────────────────────────────────────────────────────────────
    println!();
    for w in &warnings {
        println!("  {} {}", style("warn:").yellow().bold(), w);
    }
    for e in &errors {
        println!("  {} {}", style("error:").red().bold(), e);
    }

    if errors.is_empty() {
        println!(
            "\n{} Package is valid.{}",
            style("✓").green().bold(),
            if warnings.is_empty() {
                String::new()
            } else {
                format!(
                    " ({} warning{})",
                    warnings.len(),
                    if warnings.len() == 1 { "" } else { "s" }
                )
            }
        );
        Ok(())
    } else {
        bail!(
            "{} error{} found.",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" }
        )
    }
}
