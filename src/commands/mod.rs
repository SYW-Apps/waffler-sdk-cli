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

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    pub build: Option<BuildConfig>,
    #[serde(default)]
    pub module: Option<ModuleConfig>,
    #[serde(default)]
    pub process: Option<ProcessConfig>,
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
