pub mod build;
pub mod login;
pub mod logout;
pub mod namespace;
pub mod pack;
pub mod publish;
pub mod scaffold;
pub mod update;
pub mod validate;
pub mod whoami;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeVariant {
    Native,
    Wasm,
    Wasi,
    WasiService,
    Process,
}

impl RuntimeVariant {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "native" | "dll" => Ok(Self::Native),
            "wasm" => Ok(Self::Wasm),
            "wasi" => Ok(Self::Wasi),
            "wasi_service" => Ok(Self::WasiService),
            "process" => Ok(Self::Process),
            other => bail!(
                "Unknown package variant: '{}'. Available variants: native, wasm, wasi, wasi_service, process",
                other
            ),
        }
    }

    pub fn is_wasm_related(&self) -> bool {
        matches!(self, Self::Wasm | Self::Wasi | Self::WasiService)
    }

    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native)
    }
}

/// Minimal package manifest fields needed by CLI commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub id: String,
    pub namespace: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub features: ManifestFeatures,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub build: Option<BuildConfig>,
    #[serde(default)]
    pub module: Option<ModuleConfig>,
    #[serde(default)]
    pub process: Option<ProcessConfig>,
    #[serde(default)]
    pub permissions: ManifestPermissions,
}

impl PackageManifest {
    pub fn get_runtime_variant(&self) -> Result<Option<RuntimeVariant>> {
        if let Some(m) = &self.module {
            Ok(Some(RuntimeVariant::from_str(&m.runtime)?))
        } else if let Some(p) = &self.process {
            Ok(Some(RuntimeVariant::from_str(&p.runtime)?))
        } else {
            Ok(None)
        }
    }
}

/// Minimal permissions block — only the fields the CLI needs to validate.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestPermissions {
    #[serde(default)]
    pub groups: Vec<PermissionGroup>,
}

/// One approval-unit shown to the user at install time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGroup {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub rules: Vec<PermissionRule>,
}

/// A single rule within a permission group — only the `pattern.path` is checked by the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub pattern: PermissionPattern,
    #[serde(default)]
    pub effect: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPattern {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestFeatures {
    #[serde(default)]
    pub native_module: bool,
    #[serde(default)]
    pub service: bool,
    #[serde(default)]
    pub logic: bool,
    #[serde(default)]
    pub ui: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub language: String,
    /// Overrides the cargo package name passed to `cargo build -p`.
    /// Use this when the Cargo.toml `[package] name` differs from the manifest `id`.
    #[serde(default)]
    pub crate_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConfig {
    pub runtime: String,
    pub module_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    pub runtime: String,
    #[serde(default)]
    pub entrypoint: Option<String>,
}

pub fn load_manifest(package_dir: &Path) -> Result<PackageManifest> {
    let path = package_dir.join("package.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|_| anyhow::anyhow!("package.json not found in {:?}", package_dir))?;
    serde_json::from_str(&content).map_err(|e| anyhow::anyhow!("Invalid package.json: {}", e))
}
