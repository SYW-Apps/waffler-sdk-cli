use anyhow::{bail, Result};
use clap::Args;
use console::style;
use dialoguer::{Confirm, Input, Select};
use std::path::PathBuf;

use crate::auth;

#[derive(Args)]
pub struct ScaffoldArgs {
    /// Output directory for the new package (defaults to current directory)
    #[arg(default_value = ".")]
    pub output: PathBuf,
}

// ─── Wizard state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageType {
    EntitiesOnly,
    CustomLogic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    NodeTypeScript,
    Python,
    Go,
    DotNet,
    Java,
}

impl Language {
    fn label(&self) -> &str {
        match self {
            Language::Rust => "Rust",
            Language::NodeTypeScript => "Node / TypeScript",
            Language::Python => "Python",
            Language::Go => "Go",
            Language::DotNet => ".NET (C#)",
            Language::Java => "Java",
        }
    }

    fn build_id(&self) -> &str {
        match self {
            Language::Rust => "rust",
            Language::NodeTypeScript => "node",
            Language::Python => "python",
            Language::Go => "go",
            Language::DotNet => "dotnet",
            Language::Java => "java",
        }
    }

    /// Languages that can be compiled to a WASM module.
    fn supports_wasm(&self) -> bool {
        matches!(self, Language::Rust | Language::Go | Language::DotNet)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionMode {
    WasmModule,
    ExternalProcess,
}

pub async fn run(args: ScaffoldArgs) -> Result<()> {
    let theme = dialoguer::theme::ColorfulTheme::default();

    println!(
        "\n{} Waffler Package Scaffold\n",
        style("✦").cyan().bold()
    );

    // ─── Developer info ───────────────────────────────────────────────────────
    let dev_namespace: Option<String> = auth::load_credentials()
        .filter(|c| !c.is_expired())
        .and_then(|c| c.developer.namespaces.into_iter().next());

    if let Some(ref ns) = dev_namespace {
        println!(
            "  Logged in developer namespace: {}\n",
            style(ns).yellow().bold()
        );
    } else {
        println!(
            "  {} Not logged in — namespace ownership won't be validated.\n",
            style("hint:").dim()
        );
    }

    // ─── Basic metadata ───────────────────────────────────────────────────────
    let default_ns = dev_namespace
        .as_deref()
        .map(|ns| format!("{}.mypackage", ns))
        .unwrap_or_else(|| "com.example.mypackage".into());

    let namespace: String = Input::with_theme(&theme)
        .with_prompt("Package namespace (dot-separated)")
        .default(default_ns)
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.contains(' ') {
                return Err("Namespace must not contain spaces");
            }
            if input.starts_with('.') || input.ends_with('.') {
                return Err("Namespace must not start or end with a dot");
            }
            if input.split('.').count() < 2 {
                return Err("Namespace must have at least two parts (e.g. syw.mypackage)");
            }
            Ok(())
        })
        .interact_text()?;

    // Validate namespace ownership
    if let Some(ref dev_ns) = dev_namespace {
        let authorized = namespace == *dev_ns || namespace.starts_with(&format!("{}.", dev_ns));
        if !authorized {
            println!(
                "\n  {} Namespace '{}' is outside your authorized prefix '{}'.",
                style("⚠").yellow(),
                namespace,
                dev_ns
            );
            let proceed = Confirm::with_theme(&theme)
                .with_prompt("Continue anyway? (package won't validate ownership on pack)")
                .default(false)
                .interact()?;
            if !proceed {
                bail!("Scaffold cancelled.");
            }
        }
    }

    let display_name: String = Input::with_theme(&theme)
        .with_prompt("Display name")
        .default(
            namespace
                .split('.')
                .last()
                .map(|s| to_title_case(s))
                .unwrap_or_default(),
        )
        .interact_text()?;

    let description: String = Input::with_theme(&theme)
        .with_prompt("Description")
        .allow_empty(true)
        .interact_text()?;

    let publisher_default = auth::load_credentials()
        .filter(|c| !c.is_expired())
        .map(|c| c.developer.username.clone())
        .unwrap_or_else(|| "Unknown".into());

    let publisher: String = Input::with_theme(&theme)
        .with_prompt("Publisher name")
        .default(publisher_default)
        .interact_text()?;

    // ─── Package type ─────────────────────────────────────────────────────────
    println!();
    let pkg_type_idx = Select::with_theme(&theme)
        .with_prompt("What kind of package is this?")
        .items(&[
            "Entities only  — schemas, blueprints, and types (no code)",
            "Custom logic    — capabilities with compiled code",
        ])
        .default(1)
        .interact()?;

    let pkg_type = if pkg_type_idx == 0 {
        PackageType::EntitiesOnly
    } else {
        PackageType::CustomLogic
    };

    // ─── Language (only for custom logic) ────────────────────────────────────
    let (language, exec_mode): (Option<Language>, Option<ExecutionMode>) = if pkg_type == PackageType::CustomLogic {
        println!();
        let lang_choices = [
            Language::Rust,
            Language::NodeTypeScript,
            Language::Python,
            Language::Go,
            Language::DotNet,
            Language::Java,
        ];
        let lang_labels: Vec<&str> = lang_choices.iter().map(|l| l.label()).collect();
        let lang_idx = Select::with_theme(&theme)
            .with_prompt("Language")
            .items(&lang_labels)
            .default(0)
            .interact()?;
        let lang = lang_choices[lang_idx];

        let mode = if lang.supports_wasm() {
            println!();
            let mode_idx = Select::with_theme(&theme)
                .with_prompt("Execution mode")
                .items(&[
                    "WASM Module        — runs inside Waffler's sandbox (recommended, secure)",
                    "External Process   — spawned as a separate OS process (full system access)",
                ])
                .default(0)
                .interact()?;
            if mode_idx == 0 {
                ExecutionMode::WasmModule
            } else {
                ExecutionMode::ExternalProcess
            }
        } else {
            // Node/Python/Java can only run as external processes
            println!(
                "\n  {} {} packages run as external processes.",
                style("i").cyan(),
                lang.label()
            );
            ExecutionMode::ExternalProcess
        };

        (Some(lang), Some(mode))
    } else {
        (None, None)
    };

    // ─── Confirm & generate ───────────────────────────────────────────────────
    let pkg_slug = namespace.split('.').last().unwrap_or("package").to_lowercase();
    let output_dir = args.output.join(&pkg_slug);

    println!();
    println!("{}", style("─".repeat(50)).dim());
    println!("  Namespace:   {}", style(&namespace).yellow());
    println!("  Display:     {}", display_name);
    println!("  Publisher:   {}", publisher);
    let type_label = match (pkg_type, language, exec_mode) {
        (PackageType::EntitiesOnly, _, _) => "Entities only".into(),
        (_, Some(lang), Some(ExecutionMode::WasmModule)) => format!("{} → WASM Module", lang.label()),
        (_, Some(lang), Some(ExecutionMode::ExternalProcess)) => format!("{} → External Process", lang.label()),
        _ => "Custom logic".into(),
    };
    println!("  Type:        {}", type_label);
    println!("  Output:      {:?}", output_dir);
    println!("{}", style("─".repeat(50)).dim());
    println!();

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("Generate scaffold?")
        .default(true)
        .interact()?;

    if !confirmed {
        bail!("Scaffold cancelled.");
    }

    if output_dir.exists() {
        bail!("Output directory already exists: {:?}", output_dir);
    }

    // ─── Generate files ───────────────────────────────────────────────────────
    generate_scaffold(
        &output_dir,
        &namespace,
        &display_name,
        &description,
        &publisher,
        pkg_type,
        language,
        exec_mode,
    )?;

    println!(
        "\n{} Scaffold created at {:?}",
        style("✓").green().bold(),
        output_dir
    );
    println!();
    println!("  Next steps:");
    match (pkg_type, language, exec_mode) {
        (PackageType::EntitiesOnly, _, _) => {
            println!("    1. Add entity files (type.json, blueprint.json) to namespace/{}/", namespace.replace('.', "/"));
            println!("    2. Run `waffler pack` to create the installable ZIP");
        }
        (_, Some(Language::Rust), Some(ExecutionMode::WasmModule)) => {
            println!("    1. Implement your capabilities in src/lib.rs");
            println!("    2. Run `waffler build` to compile the WASM module");
            println!("    3. Run `waffler pack` to create the installable ZIP");
        }
        _ => {
            println!("    1. Implement your capabilities");
            println!("    2. Run `waffler build`");
            println!("    3. Run `waffler pack`");
        }
    }

    Ok(())
}

// ─── Scaffold file generation ─────────────────────────────────────────────────

fn generate_scaffold(
    output_dir: &std::path::Path,
    namespace: &str,
    display_name: &str,
    description: &str,
    publisher: &str,
    pkg_type: PackageType,
    language: Option<Language>,
    exec_mode: Option<ExecutionMode>,
) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;

    // Create namespace/ directory structure
    let ns_path: PathBuf = namespace.split('.').collect();
    let ns_dir = output_dir.join("namespace").join(&ns_path);
    std::fs::create_dir_all(&ns_dir)?;

    // namespace/README.md for developer guidance
    std::fs::write(
        output_dir.join("namespace").join("README.md"),
        format!(
            "# Namespace: {namespace}\n\n\
             This directory mirrors the Waffler namespace tree.\n\
             Each subdirectory is an entity (schema, blueprint, etc.).\n\n\
             - Add `type.json` to a directory to define a schema/UDT\n\
             - Add `blueprint.json` to a directory to define a blueprint\n\
             - `segment.json` files are auto-generated by `waffler pack`\n\n\
             Package namespace root: `namespace/{ns_path}/`\n",
            namespace = namespace,
            ns_path = namespace.replace('.', "/"),
        ),
    )?;

    // Write package.json
    let manifest_json = build_manifest_json(
        namespace,
        display_name,
        description,
        publisher,
        pkg_type,
        language,
        exec_mode,
    );
    std::fs::write(
        output_dir.join("package.json"),
        serde_json::to_string_pretty(&manifest_json)?,
    )?;

    // Language-specific scaffold
    match (pkg_type, language, exec_mode) {
        (PackageType::EntitiesOnly, _, _) => {
            // Add an example schema entity
            let example_dir = ns_dir.join("ExampleEntity");
            std::fs::create_dir_all(&example_dir)?;
            std::fs::write(
                example_dir.join("type.json"),
                EXAMPLE_TYPE_JSON.replace("{namespace}", namespace),
            )?;
        }

        (_, Some(Language::Rust), Some(ExecutionMode::WasmModule)) => {
            scaffold_rust_wasm(output_dir, namespace)?;
        }

        (_, Some(Language::Rust), Some(ExecutionMode::ExternalProcess)) => {
            scaffold_rust_service(output_dir, namespace)?;
        }

        (_, Some(Language::NodeTypeScript), _) => {
            scaffold_node(output_dir, namespace)?;
        }

        (_, Some(Language::Python), _) => {
            scaffold_python(output_dir, namespace)?;
        }

        _ => {
            // Generic placeholder for unsupported combinations
            std::fs::write(
                output_dir.join("README.md"),
                format!(
                    "# {display_name}\n\nNamespace: `{namespace}`\n\nImplement your package here.\n"
                ),
            )?;
        }
    }

    Ok(())
}

fn build_manifest_json(
    namespace: &str,
    display_name: &str,
    description: &str,
    publisher: &str,
    pkg_type: PackageType,
    language: Option<Language>,
    exec_mode: Option<ExecutionMode>,
) -> serde_json::Value {
    let pkg_id = namespace.to_string();
    let crate_name = namespace.replace('.', "_");

    let mut features = serde_json::json!({ "logic": false });
    let mut module_field = serde_json::Value::Null;
    let mut process_field = serde_json::Value::Null;
    let build_language_owned = language.map(|l| l.build_id().to_string())
        .unwrap_or_else(|| "waffler_native".to_string());
    let build_language = build_language_owned.as_str();

    match (pkg_type, exec_mode) {
        (PackageType::EntitiesOnly, _) => {}
        (_, Some(ExecutionMode::WasmModule)) => {
            features = serde_json::json!({ "logic": true, "native_module": true });
            module_field = serde_json::json!({
                "runtime": "wasm",
                "module_path": format!("{}.wasm", crate_name)
            });
        }
        (_, Some(ExecutionMode::ExternalProcess)) => {
            features = serde_json::json!({ "logic": true, "service": true });
            let ext = if cfg!(windows) { ".exe" } else { "" };
            process_field = serde_json::json!({
                "runtime": build_language,
                "entrypoint": format!("{}{}", crate_name, ext)
            });
        }
        _ => {}
    }

    let mut manifest = serde_json::json!({
        "id": pkg_id,
        "namespace": namespace,
        "version": "0.1.0",
        "display_name": display_name,
        "description": description,
        "publisher": publisher,
        "manifest_version": "2.0",
        "features": features,
        "build": { "language": build_language },
        "supported_os": ["windows", "linux", "darwin"],
        "permissions": { "requested": [] },
        "capabilities": []
    });

    if !module_field.is_null() {
        manifest["module"] = module_field;
    }
    if !process_field.is_null() {
        manifest["process"] = process_field;
    }

    manifest
}

// ─── Language scaffold templates ──────────────────────────────────────────────

fn scaffold_rust_wasm(output_dir: &std::path::Path, namespace: &str) -> Result<()> {
    let crate_name = namespace.replace('.', "_");

    std::fs::write(
        output_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"

[lib]
name = "{crate_name}"
path = "src/lib.rs"
crate-type = ["cdylib"]

[dependencies]
# Waffler host import bindings — provides http_request, capability registration, etc.
waffler-native-module = {{ path = "../../sdk/rust/waffler-native-module" }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
"#,
            crate_name = crate_name
        ),
    )?;

    std::fs::create_dir_all(output_dir.join("src"))?;
    std::fs::write(
        output_dir.join("src").join("lib.rs"),
        format!(
            r#"//! {namespace} — Waffler WASM Native Module
//!
//! Implement your capabilities here. Each capability receives a JSON payload
//! and returns a JSON result to the Waffler runtime.
//!
//! Build with:  cargo build --target wasm32-unknown-unknown --release
//! Pack with:   waffler pack

use serde_json::{{json, Value}};

/// Entry point called by the Waffler WASM runtime for each capability invocation.
///
/// # Arguments
/// * `capability_id` - The capability ID from package.json (e.g. "mypackage.do_something")
/// * `inputs` - JSON object containing the capability inputs
///
/// # Returns
/// A JSON object with the capability outputs, or an error string.
#[no_mangle]
pub extern "C" fn execute_capability(
    capability_id_ptr: *const u8,
    capability_id_len: usize,
    inputs_ptr: *const u8,
    inputs_len: usize,
) -> *mut u8 {{
    let cap_id = unsafe {{
        let slice = std::slice::from_raw_parts(capability_id_ptr, capability_id_len);
        std::str::from_utf8_unchecked(slice)
    }};
    let inputs_bytes = unsafe {{ std::slice::from_raw_parts(inputs_ptr, inputs_len) }};
    let inputs: Value = serde_json::from_slice(inputs_bytes).unwrap_or(Value::Null);

    let result = match cap_id {{
        "example.greet" => handle_greet(inputs),
        _ => Err(format!("Unknown capability: {{cap_id}}")),
    }};

    let response = match result {{
        Ok(v) => json!({{ "ok": true, "data": v }}),
        Err(e) => json!({{ "ok": false, "error": e }}),
    }};

    let bytes = serde_json::to_vec(&response).unwrap_or_default();
    // Caller is responsible for freeing this allocation
    let ptr = bytes.as_ptr() as *mut u8;
    std::mem::forget(bytes);
    ptr
}}

fn handle_greet(inputs: Value) -> Result<Value, String> {{
    let name = inputs["name"].as_str().unwrap_or("World");
    Ok(json!({{
        "message": format!("Hello, {{name}}!")
    }}))
}}
"#,
            namespace = namespace
        ),
    )?;

    // .cargo/config.toml for cross-compilation hints
    std::fs::create_dir_all(output_dir.join(".cargo"))?;
    std::fs::write(
        output_dir.join(".cargo").join("config.toml"),
        r#"[build]
# Uncomment to build for WASM by default:
# target = "wasm32-unknown-unknown"
"#,
    )?;

    Ok(())
}

fn scaffold_rust_service(output_dir: &std::path::Path, namespace: &str) -> Result<()> {
    let crate_name = namespace.replace('.', "_");

    std::fs::write(
        output_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{crate_name}"
path = "src/main.rs"

[dependencies]
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
tokio = {{ version = "1", features = ["full"] }}
"#,
            crate_name = crate_name
        ),
    )?;

    std::fs::create_dir_all(output_dir.join("src"))?;
    std::fs::write(
        output_dir.join("src").join("main.rs"),
        format!(
            r#"//! {namespace} — Waffler External Process Package
//!
//! This binary is spawned by the Waffler runtime as an external process.
//! It communicates via stdin/stdout using the Waffler IPC protocol.

#[tokio::main]
async fn main() {{
    eprintln!("[{namespace}] Package process started");
    // TODO: Initialize Waffler IPC connection and handle capability requests
    // See the Waffler SDK documentation for the IPC protocol details.
    loop {{
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }}
}}
"#,
            namespace = namespace
        ),
    )?;

    Ok(())
}

fn scaffold_node(output_dir: &std::path::Path, namespace: &str) -> Result<()> {
    let app_dir = output_dir.join("app");
    let src_dir = app_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;

    std::fs::write(
        app_dir.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": namespace,
            "version": "0.1.0",
            "main": "dist/index.js",
            "scripts": {
                "build": "tsc",
                "start": "node dist/index.js"
            },
            "dependencies": {},
            "devDependencies": {
                "typescript": "^5.0.0"
            }
        }))?,
    )?;

    std::fs::write(
        app_dir.join("tsconfig.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "compilerOptions": {
                "target": "ES2020",
                "module": "commonjs",
                "outDir": "dist",
                "strict": true
            },
            "include": ["src"]
        }))?,
    )?;

    std::fs::write(
        src_dir.join("index.ts"),
        format!(
            r#"// {namespace} — Waffler External Process Package (Node/TypeScript)
// This process is spawned by the Waffler runtime.

console.error('[{namespace}] Package process started');

// TODO: Initialize Waffler IPC and handle capability requests.
// See the Waffler SDK documentation for the IPC protocol details.
"#,
            namespace = namespace
        ),
    )?;

    Ok(())
}

fn scaffold_python(output_dir: &std::path::Path, namespace: &str) -> Result<()> {
    std::fs::write(
        output_dir.join("main.py"),
        format!(
            r#"#!/usr/bin/env python3
"""
{namespace} — Waffler External Process Package (Python)
This script is spawned by the Waffler runtime as an external process.
"""

import sys

def main():
    print(f"[{namespace}] Package process started", file=sys.stderr)
    # TODO: Initialize Waffler IPC and handle capability requests.
    # See the Waffler SDK documentation for the IPC protocol details.

if __name__ == "__main__":
    main()
"#,
            namespace = namespace
        ),
    )?;

    std::fs::write(
        output_dir.join("requirements.txt"),
        "# Add your dependencies here\n",
    )?;

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn to_title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

const EXAMPLE_TYPE_JSON: &str = r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "id": "{namespace}.ExampleEntity",
  "title": "ExampleEntity",
  "description": "An example entity — replace with your own schema.",
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "description": "Example string property"
    },
    "value": {
      "type": "number",
      "description": "Example numeric property"
    }
  },
  "required": ["name"]
}
"#;
