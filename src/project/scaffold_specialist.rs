//! `sdk_cli::scaffold-specialist` — rendering a new package project.
//!
//! Spec: `sdk_cli::scaffold-specialist` / `iscaffold-specialist` / `scaffold_specialist_impl`.
//!
//! PURE, AND THE PURITY IS WHAT MAKES IT TESTABLE AGAINST `pack`. A rendered project can be handed
//! straight to the manifest compiler in a test and refused there if it is wrong, which is the only
//! way a scaffold's claim about the current format stays true. The previous scaffold wrote the
//! legacy manifest model and had been generating unpublishable projects for as long as the registry
//! had refused them — with nothing in the tree able to notice, because nothing tested the two
//! against each other.

use super::types::RenderedFile;

/// The core CONTRACT range a generated project declares.
///
/// PINNED RATHER THAN OMITTED. A bundle declaring none installs UNCHECKED behind a warning nobody
/// reads, and a scaffold is the one place where the right declaration costs the developer nothing —
/// so omitting it here would be choosing the silent path on their behalf.
pub const SCAFFOLD_CORE_COMPATIBILITY: &str = "^0.1";

/// Render a complete package project.
///
/// `sdk_path` is where the Waffler SDK crates live, relative to the generated project or absolute.
/// IT IS A PARAMETER BECAUSE THE SDK IS NOT PUBLISHED TO A REGISTRY, and a scaffold that emitted a
/// dependency line pointing nowhere would generate a project that cannot build — the exact failure
/// this component exists to prevent, moved from the manifest into the crate manifest.
pub fn render_project(fqid: &str, version: &str, description: &str, sdk_path: &str) -> Vec<RenderedFile> {
    // ONE DERIVATION, STATED ONCE, and now used in ONE place rather than two. The generated manifest
    // no longer restates where the build's output goes — it says `fromBuild` and lets the build tool
    // answer, which is what makes the generated project pack on every platform instead of only on the
    // one it was created on.
    let crate_name = fqid.replace(['.', '-'], "_");

    vec![
        RenderedFile {
            relative_path: super::types::MANIFEST_FILE.to_string(),
            contents: render_manifest(fqid, version, description),
        },
        RenderedFile { relative_path: "Cargo.toml".into(), contents: render_cargo_toml(&crate_name, version, description, sdk_path) },
        RenderedFile { relative_path: "src/lib.rs".into(), contents: render_lib_rs(fqid, version) },
        RenderedFile { relative_path: "README.md".into(), contents: render_readme(fqid) },
        RenderedFile { relative_path: ".gitignore".into(), contents: "/target\n*.zip\n".into() },
    ]
}

fn render_manifest(fqid: &str, version: &str, description: &str) -> String {
    let capability = "echo";
    format!(
        r#"{{
  "fqid": "{fqid}",
  "version": "{version}",
  "description": "{description}",

  "//": "The core CONTRACT range this package is built against. A bundle that declares none installs UNCHECKED behind a warning nobody reads.",
  "coreCompatibility": "{SCAFFOLD_CORE_COMPATIBILITY}",

  "build": {{
    "//": "Named, never inferred. Deriving a crate name from the fqid worked until the two differed, and then it built the wrong crate and packed its artifact under the right name.",
    "manifestPath": "Cargo.toml"
  }},

  "//artifacts": "`fromBuild` asks the build tool where its output went. A literal path would encode two things the build tool owns -- the target directory, which CARGO_TARGET_DIR and workspaces move, and the platform-specific file name -- and both were wrong the first time this was run for real.",
  "artifacts": [
    {{
      "fromBuild": true,
      "kind": "Dll",
      "entryPoint": "wf_init"
    }}
  ],

  "//capabilities": "This package serves '{capability}' over the bus. It is served by the handler in src/lib.rs, not declared here — this list is for capabilities a HOST registers, such as runtime primitives.",
  "capabilities": [],

  "//dependencies": "Other packages this one needs: {{ \"fqid\": \"...\", \"version\": \"^1.0\" }}. A RANGE, never a pin. Add \"optional\": true for one the package can run without -- the default is REQUIRED, and a required dependency that is missing stops this package starting at boot rather than letting it run broken.",
  "dependencies": [],

  "//middleware": "Interceptors core runs around routed bus calls: {{ \"id\": \"auth\", \"target\": \"syw.system.*\", \"needsHeaders\": true, \"filters\": {{ \"capability\": \"admin.*\" }} }}. `target` absent intercepts EVERY routed call. Declare the NARROWEST filters that are still correct: core assembles the chain in-process, so a call your filters exclude never crosses into this package at all -- filtering is how an interceptor avoids being asked about traffic it would only wave through. Two keys are refused inside `filters`: `source` (a node owner-tags it at registration) and `target` (it has its own field). AND DECLARING ANY MIDDLEWARE COMMITS YOU TO A GRANT: core registers each one on the global bus chain, that is gated on `bus:register_middleware`, and failing it is FATAL -- so a package declaring middleware without a permission group requesting that grant will not run.",
  "middleware": [],
  "permissionGroups": [],
  "fastLaneRequests": [],
  "uiPlugins": []
}}
"#
    )
}

fn render_cargo_toml(crate_name: &str, version: &str, description: &str, sdk_path: &str) -> String {
    format!(
        r#"[package]
name = "{crate_name}"
version = "{version}"
edition = "2021"
description = "{description}"

# STANDALONE CRATE, detached from any parent workspace.
#
# Without this empty table, a crate created inside a checkout that lists its path in neither
# `members` nor `exclude` refuses to build at all -- "current package believes it's in a workspace
# when it's not". Every package in the Waffler tree carries it.
[workspace]

[lib]
# cdylib is what a node loads; rlib is what `cargo test` links. Both, so the package is testable.
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[dependencies]
# The Waffler SDK is not published to a package registry yet, so these are PATH dependencies
# pointing at a Waffler checkout. Change them if you move this project.
waffler_sdk = {{ path = "{sdk_path}/sdk/rust" }}
waffler_shared = {{ path = "{sdk_path}/shared" }}
async-trait = "0.1"
serde_json = "1"
rmp-serde = "1"
"#
    )
}

fn render_lib_rs(fqid: &str, version: &str) -> String {
    format!(
        r#"//! `{fqid}` -- a Waffler package.
//!
//! It serves one capability, `echo`, which returns the payload it was handed along with this
//! package's own fqid and version.
//!
//! ## WHY THE GENERATED PACKAGE ANSWERS RATHER THAN MERELY LOADING
//!
//! "Installed" and "working" are different observations. A package with no capabilities leaves only
//! a row in `packages:list` as evidence it exists -- which a records-only install would produce just
//! as well, so an install that wrote the record and failed to load the artifact would look
//! identical. Calling `echo` and getting the right bytes back proves the ARTIFACT LOADED and is
//! EXECUTING. Keep that property as you replace this: a package whose first test can only prove it
//! installed teaches you to stop one step too early.

use std::sync::Arc;

use waffler_sdk::native_guest::GuestEnv;
use waffler_sdk::sdk_context::{{CapabilityContext, CommandHandler}};
use waffler_shared::{{Response, WafflerError}};

pub const FQID: &str = "{fqid}";
pub const VERSION: &str = "{version}";

/// The capability that makes an install observable.
pub const CAP_ECHO: &str = "echo";

#[derive(Debug)]
struct Handler;

/// Build the reply body for one call.
///
/// SPLIT OUT SO IT IS TESTABLE WITHOUT A BUS: `CapabilityContext` references SDK-private types, so a
/// unit test cannot construct one. The interesting part is what comes back, and that is a pure
/// function of the capability name and the payload.
pub fn reply_body(capability: &str, payload: &[u8]) -> serde_json::Value {{
    if capability == CAP_ECHO {{
        serde_json::json!({{
            "package": FQID, "version": VERSION, "capability": capability,
            "echo": decode_payload(payload), "bytes": payload.len(),
        }})
    }} else {{
        // An unknown capability is NAMED rather than silently echoed. A package that answered
        // everything would report success for a caller that asked for something it does not serve.
        serde_json::json!({{
            "package": FQID, "version": VERSION,
            "error": "unknown capability", "capability": capability,
        }})
    }}
}}

/// Read the request payload back into a value.
///
/// JSON IS TRIED FIRST, AND THE ORDER IS LOAD-BEARING. `rmp_serde::from_slice` IGNORES trailing
/// bytes, and `{{` is 0x7b -- a valid MessagePack positive fixint -- so MessagePack-first decodes
/// `{{"from":"curl"}}` as the number 123 and throws the rest away. It answers confidently and
/// wrongly. `serde_json::from_slice` rejects trailing data, so it cannot make the mirror-image
/// mistake; and a MessagePack string carries a header byte that is not valid UTF-8, so it never
/// reaches the JSON parser at all.
fn decode_payload(payload: &[u8]) -> serde_json::Value {{
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {{
        return v;
    }}
    if let Ok(v) = rmp_serde::from_slice::<serde_json::Value>(payload) {{
        return v;
    }}
    match std::str::from_utf8(payload) {{
        Ok(text) => serde_json::Value::String(text.to_string()),
        Err(_) => serde_json::Value::Null,
    }}
}}

#[async_trait::async_trait]
impl CommandHandler for Handler {{
    async fn on_command(&self, ctx: CapabilityContext) -> Result<Response, WafflerError> {{
        let capability = ctx.headers.capability.clone().unwrap_or_default();
        // MESSAGEPACK, NAMED -- the bus's own encoding, which is what a caller decodes. A reply
        // encoded as JSON is opaque to the bus either way, so nothing refuses it; the failure
        // appears only at the CALLER, as a decoder reporting extra bytes for a valid JSON body.
        let payload = rmp_serde::to_vec_named(&reply_body(&capability, &ctx.payload)).map_err(|e| WafflerError {{
            category: "package".into(),
            code: Some("EncodeFailed".into()),
            message: e.to_string(),
        }})?;
        Ok(Response {{ payload, headers: ctx.headers }})
    }}
}}

/// The composition root.
fn init(_env: GuestEnv) -> Result<Arc<dyn CommandHandler>, WafflerError> {{
    println!("{{FQID}} init version={{VERSION}}");
    Ok(Arc::new(Handler))
}}

waffler_sdk::waffler_direct_package!(init);

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn echo_returns_a_messagepack_payload_as_the_value_it_encodes() {{
        // THE ENCODING THAT ACTUALLY ARRIVES. A plain string reaches the handler as a MessagePack
        // fixstr -- a header byte plus the text -- not as raw text.
        let wire = rmp_serde::to_vec(&"hello").unwrap();
        let body = reply_body(CAP_ECHO, &wire);
        assert_eq!(body["echo"], "hello");
        // The identity is in the reply, so a caller can tell WHICH package answered. A reply that
        // only said "hello" would prove something echoed, not that this artifact did.
        assert_eq!(body["package"], FQID);
        assert_eq!(body["version"], VERSION);
    }}

    #[test]
    fn an_unknown_capability_is_named_rather_than_answered() {{
        let body = reply_body("not_a_capability", b"x");
        assert_eq!(body["error"], "unknown capability");
        assert!(body.get("echo").is_none(), "it must not look like a successful echo");
    }}
}}
"#
    )
}

fn render_readme(fqid: &str) -> String {
    format!(
        r#"# {fqid}

A Waffler package.

## The three commands that take this to a node

```
waffler build          # release profile -- a debug module exceeds core's inline custody cap
waffler pack           # writes <fqid>.zip: /.manifest, /artifact/<name>, /namespace/<uuid>.json
waffler publish        # uploads it; the registry signs what it accepts
```

`waffler validate` runs every manifest check without building, which is the cheap answer while you
are editing `waffler.json`.

## Which registry a command talks to

Resolved in this order, and every command prints which one it used and why:

1. `--registry <url>` on the command
2. `WAFFLER_REGISTRY` in the environment
3. the default persisted by `waffler use <url>`
4. the built-in public registry

Sign in per registry -- `waffler login` -- because two registries are two identity providers with
two ideas of who owns which namespace. `waffler whoami` reports the identity for one registry, and
names which.

## What gets uploaded

An UNSIGNED bundle. The registry is the signing authority on that path and refuses an upload that
already carries a registry signature: it cannot safely decide which trailing bytes are signature and
which are content, and guessing wrong publishes a corrupt package successfully.
"#
    )
}

#[cfg(test)]
#[path = "scaffold_specialist.test.rs"]
mod tests;
