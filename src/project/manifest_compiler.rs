//! `sdk_cli::manifest-compiler` — what a developer AUTHORED becomes what a node INSTALLS.
//!
//! Spec: `sdk_cli::manifest-compiler` / `imanifest-compiler` / `manifest_compiler_impl`.
//!
//! PURE. No clock, no disk, no network — which is what makes every refusal testable from a literal
//! and every compilation testable without a build.
//!
//! EVERY REFUSAL HERE MIRRORS A REGISTRY RULE, AND THE REGISTRY IS THE AUTHORITY. This side exists
//! so a developer learns about a malformed manifest in a second rather than after a release build
//! and a hundred-megabyte upload. Where the two disagree the registry is right and this is the bug,
//! so a check is added here only when the registry already has it — never the reverse. A local
//! check the registry lacks is a rule enforced against honest developers and against nobody else.

use serde_json::json;

use super::types::{AuthoredPackage, LocatedArtifact, Violation};

/// The artifact kind that makes a package load in-process.
pub const KIND_DLL: &str = "Dll";
/// The artifact kind the frontend loads.
pub const KIND_UI_BUNDLE: &str = "UiBundle";

/// Check an authored manifest against the rules the registry will apply.
///
/// ALL VIOLATIONS, NOT THE FIRST. Fixing a manifest one refused field per build is how a
/// five-minute correction becomes an afternoon.
pub fn validate_authored(authored: &AuthoredPackage) -> Vec<Violation> {
    let mut violations = Vec::new();

    // The fqid is the registry namespace, the install key AND the seed of the deterministic package
    // uuid, so a malformed one is wrong in three places at once and none of them says so.
    if authored.fqid.trim().is_empty() {
        violations.push(Violation::new("fqid", "must not be empty"));
    } else if !authored.fqid.contains('.') {
        violations.push(Violation::new(
            "fqid",
            format!("'{}' is not a fully-qualified id — it must be dotted, like 'syw.probe.echo'", authored.fqid),
        ));
    }

    if semver::Version::parse(&authored.version).is_err() {
        violations.push(Violation::new(
            "version",
            format!("'{}' is not a SemVer version; the registry parses it too, and one it cannot order is one the resolver cannot answer a range with", authored.version),
        ));
    }

    if let Some(compat) = authored.core_compatibility.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        // A REQUIREMENT, NOT A VERSION. `^0.1` is the shape; `0.1` is a narrower one that still
        // parses; a string that parses as neither installs UNCHECKED while looking declared, which
        // is the worst of the three outcomes.
        if semver::VersionReq::parse(compat).is_err() {
            violations.push(Violation::new(
                "coreCompatibility",
                format!("'{compat}' is not a SemVer requirement (try '^0.1'); a value that parses as neither a range nor a version installs UNCHECKED while looking declared"),
            ));
        }
    }

    // Every cross-reference below resolves against this set.
    //
    // A `fromBuild` artifact's bundle name is not known until the build tool has been asked, so an
    // EXPLICIT `name` is what makes it referenceable from a capability or a ui plugin. Without one it
    // contributes nothing here, and a declaration naming it is refused with the list of what IS
    // declared -- which is the right message, because the fix is to give it a name.
    let names: std::collections::HashSet<String> =
        authored.artifacts.iter().filter_map(declared_name_of).collect();

    for (i, artifact) in authored.artifacts.iter().enumerate() {
        let at = format!("artifacts[{i}]");
        // EXACTLY ONE AUTHORITY PER ARTIFACT. `path` says the developer knows where the file is;
        // `fromBuild` says the build tool does. Both together is a manifest that could disagree with
        // itself, and neither is one that names nothing at all.
        match (artifact.path.as_deref().map(str::trim).filter(|p| !p.is_empty()), artifact.from_build) {
            (Some(path), false) => {
                if is_escaping_path(path) {
                    // A bundle assembled from files outside the project is one nobody can reproduce
                    // from the repository.
                    violations.push(Violation::new(
                        format!("{at}.path"),
                        format!("'{path}' escapes the project directory; an artifact must live inside the project that declares it"),
                    ));
                }
            }
            (None, true) => {}
            (Some(_), true) => violations.push(Violation::new(
                at.clone(),
                "declares both `path` and `fromBuild`; exactly one authority answers where an artifact is",
            )),
            (None, false) => violations.push(Violation::new(
                at.clone(),
                "declares neither `path` nor `fromBuild`, so nothing says where this artifact is",
            )),
        }
        match artifact.kind.as_str() {
            KIND_DLL => {
                // A process host with no symbol to call loads a module that does nothing and
                // reports success.
                if artifact.entry_point.as_deref().unwrap_or("").trim().is_empty() {
                    violations.push(Violation::new(
                        format!("{at}.entryPoint"),
                        "a Dll must declare an entry point (the Rust SDK's is 'wf_init'); a host with no symbol to call loads a module that does nothing and reports success",
                    ));
                }
            }
            KIND_UI_BUNDLE => {}
            other => violations.push(Violation::new(
                format!("{at}.kind"),
                format!("'{other}' is not an artifact kind; expected '{KIND_DLL}' or '{KIND_UI_BUNDLE}'"),
            )),
        }
    }

    for (i, capability) in authored.capabilities.iter().enumerate() {
        let at = format!("capabilities[{i}]");
        if capability.fqid.trim().is_empty() {
            violations.push(Violation::new(format!("{at}.fqid"), "must not be empty"));
        }
        // THE REGISTRY'S BUNDLE READER ALIGNS DECLARED ARTIFACTS TO ZIP ENTRIES IN BOTH DIRECTIONS,
        // so a capability pointing at nothing is refused there too — with a message about a zip
        // entry rather than about the field the developer wrote.
        if !names.contains(&capability.artifact) {
            violations.push(Violation::new(
                format!("{at}.artifact"),
                format!("names '{}', which no declared artifact provides (declared: {})", capability.artifact, name_list(&names)),
            ));
        }
        if capability.kind == "RuntimePrimitive" && capability.symbol.as_deref().unwrap_or("").trim().is_empty() {
            violations.push(Violation::new(
                format!("{at}.symbol"),
                "a RuntimePrimitive must name the C symbol the runtime binds",
            ));
        }
    }

    for (i, plugin) in authored.ui_plugins.iter().enumerate() {
        let at = format!("uiPlugins[{i}]");
        if plugin.id.trim().is_empty() {
            violations.push(Violation::new(format!("{at}.id"), "must not be empty"));
        }
        match authored.artifacts.iter().find(|a| declared_name_of(a).as_deref() == Some(plugin.artifact.as_str())) {
            None => violations.push(Violation::new(
                format!("{at}.artifact"),
                format!("names '{}', which no declared artifact provides (declared: {})", plugin.artifact, name_list(&names)),
            )),
            Some(artifact) if artifact.kind != KIND_UI_BUNDLE => violations.push(Violation::new(
                format!("{at}.artifact"),
                format!("names '{}', which is declared as kind '{}' rather than '{KIND_UI_BUNDLE}'", plugin.artifact, artifact.kind),
            )),
            Some(_) => {}
        }
        // THE SLOT IS DELIBERATELY NOT CHECKED against any list of region names. A packer has no
        // business knowing the frontend's regions, and coupling it to that list makes every new
        // region a change here.
    }

    for (i, dependency) in authored.dependencies.iter().enumerate() {
        let at = format!("dependencies[{i}]");
        if dependency.fqid.trim().is_empty() {
            violations.push(Violation::new(format!("{at}.fqid"), "must not be empty"));
        }
        if semver::VersionReq::parse(&dependency.version).is_err() {
            violations.push(Violation::new(
                format!("{at}.version"),
                format!("'{}' is not a SemVer requirement (try '^1.0'); the registry resolves a RANGE against what is published", dependency.version),
            ));
        }
    }

    violations
}

/// The name a declaration is referenceable by BEFORE anything has been located.
///
/// `None` for a `fromBuild` artifact with no explicit name: what the build tool will call it is not
/// knowable at validation time, and inventing a plausible name here would let a capability reference
/// resolve against a guess.
fn declared_name_of(a: &super::types::DeclaredArtifact) -> Option<String> {
    if let Some(name) = a.name.as_deref().filter(|n| !n.is_empty()) {
        return Some(name.to_string());
    }
    // The last segment of a declared path, under either separator: a manifest is written on one
    // platform and packed on another, and a declaration spelled with backslashes is a portability bug
    // rather than a path — but it must still resolve to the same name here as it does at pack time.
    let path = a.path.as_deref()?;
    Some(path.rsplit(['/', '\\']).next().unwrap_or(path).to_string())
}

/// A relative path that climbs out of the project, or is absolute.
///
/// CHECKED ON THE DECLARATION, not on the resolved path, because the declaration is what a reviewer
/// reads in a diff. A `..` that resolves back inside the project by luck is still a declaration
/// nobody can check by looking at it.
fn is_escaping_path(path: &str) -> bool {
    let p = std::path::Path::new(path);
    // `has_root` AS WELL AS `is_absolute`, and the difference is a real portability trap rather than
    // belt-and-braces. On Windows `/usr/lib/x.so` is NOT absolute — it has no drive — so `is_absolute`
    // answers false, while `Path::join` still discards the project directory and yields a
    // drive-relative `\usr\lib\x.so`. The declaration escapes the project on the platform where the
    // check that was supposed to catch it says nothing. Caught by the test that asserts all three
    // spellings, run on Windows.
    if p.is_absolute() || p.has_root() {
        return true;
    }
    // A Windows drive-relative spelling (`C:foo`) is absolute in intent and is caught by neither of
    // those on unix, so the colon is checked directly. A manifest is written on one platform and
    // packed on another, so every spelling has to be refused everywhere.
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return true;
    }
    p.components().any(|c| matches!(c, std::path::Component::ParentDir))
}

fn name_list(names: &std::collections::HashSet<String>) -> String {
    if names.is_empty() {
        return "none".to_string();
    }
    let mut sorted: Vec<&str> = names.iter().map(std::string::String::as_str).collect();
    sorted.sort_unstable();
    sorted.join(", ")
}

/// Produce the `/.manifest` declarations.
///
/// THE ARTIFACTS ARRAY IS LEFT ABSENT — its hashes are facts about bytes the writer has not written
/// yet, and a hash copied from anywhere else is a claim a later step could contradict.
pub fn compile_manifest_body(authored: &AuthoredPackage, located: &[LocatedArtifact]) -> serde_json::Value {
    // HOSTING MODE IS DERIVED, NEVER DECLARED. It is a fact about what was built; the previous tool
    // asked for it as `module.runtime` and let a project claim a mode its artifacts could not
    // support. A package shipping only a UI bundle stays HOSTED and starts nothing, because a UI
    // bundle is loaded by the frontend and not by a process host.
    let has_dll = located.iter().any(|a| a.kind == KIND_DLL);
    let hosting_mode = if has_dll { "DIRECT" } else { "HOSTED" };

    let artifact_ref = |name: &str| -> serde_json::Value {
        match located.iter().find(|a| a.name == name) {
            // The hash is filled in by the writer; here the reference exists to name the artifact a
            // declaration is realized by.
            Some(a) => json!({ "name": a.name, "kind": a.kind, "hash": "", "entry_point": a.entry_point }),
            None => json!({ "name": name, "kind": KIND_DLL, "hash": "", "entry_point": null }),
        }
    };

    let capabilities: Vec<serde_json::Value> = authored
        .capabilities
        .iter()
        .map(|c| {
            let inputs: Vec<serde_json::Value> = c
                .inputs
                .iter()
                .map(|p| {
                    json!({
                        "key": p.key, "label": p.key,
                        "type": { "kind": p.r#type.clone().unwrap_or_else(|| "Dynamic".into()) },
                        "ui": null, "constraints": null, "default_value": null,
                        "is_self": null, "connector_generation": null, "provided_tags": null
                    })
                })
                .collect();
            json!({
                "fqid": c.fqid,
                "version": authored.version,
                "kind": c.kind,
                "target_host": c.target_host,
                "artifact_ref": artifact_ref(&c.artifact),
                "inputs": inputs,
                "output": null,
                "metadata": {
                    "display_name": c.fqid,
                    "node_descriptor": c.symbol.as_ref().map(|s| json!({ "c_symbol": s, "ffi_signature": "wf_prim_v1" })),
                }
            })
        })
        .collect();

    let ui_plugins: Vec<serde_json::Value> = authored
        .ui_plugins
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                // The PLUGIN's version tracks the package's: they ship together, and a separately
                // numbered UI is a second thing to keep in step for no gain.
                "version": authored.version,
                "slot": p.slot,
                "entry_artifact": artifact_ref(&p.artifact),
                // Absent rather than emitted empty: absent means "this plugin declares no
                // contributions here", and an empty object would read as a declaration listing none.
                "contributes": null
            })
        })
        .collect();

    let dependencies: Vec<serde_json::Value> = authored
        .dependencies
        .iter()
        // `kind: "package"` is what core's own dependency resolver stamps on a package-level
        // transitive entry, and what the registry reads back as "<fqid>@<version>".
        //
        // `optional` IS ALWAYS WRITTEN, including when false. Core's decoder defaults an absent flag
        // to required, so omitting it would produce the same meaning — but then a reader of the
        // manifest cannot tell "this author considered it and chose required" from "this manifest
        // predates the field". Writing it makes the declaration say what it means, which is the
        // whole reason the field exists.
        .map(|d| json!({
            "fqid": d.fqid,
            "version": d.version,
            "kind": "package",
            "optional": d.optional,
        }))
        .collect();

    let fast_lane_requests: Vec<serde_json::Value> = authored
        .fast_lane_requests
        .iter()
        // The reviewable ASK: required and Pending. The trigger-aware approval pass, not the
        // manifest, materializes any grant.
        .map(|r| json!({ "target": r.target, "secure": r.secure, "required": true, "review": "Pending" }))
        .collect();

    let mut body = json!({
        "fqid": authored.fqid,
        "version": authored.version,
        "hosting_mode": hosting_mode,
        "permission_groups": authored.permission_groups,
        "dependencies": dependencies,
        "capabilities": capabilities,
        "ui_plugins": ui_plugins,
        "middleware": [],
        "fast_lane_requests": fast_lane_requests,
        "enabled": true
    });

    // RUNTIME-MANAGED FIELDS ARE OMITTED, not emitted empty. `identity`, `approved_group_ids` and
    // `fast_lane_grants` are minted by a node; writing them blank would make a bundle that declares
    // nothing indistinguishable from one whose author wrote an empty value, and core's own warnings
    // depend on telling those apart.

    if let Some(compat) = authored.core_compatibility.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        // Emitted ONLY when declared — absent must stay absent so core's warning tells the truth
        // about which case it is looking at.
        body["core_compatibility"] = json!(compat);
    }

    body
}

/// The deterministic package uuid: `uuidv5(NAMESPACE_OID, "waffler.package:<fqid>")`.
///
/// COPIED FROM THE RUNTIME RATHER THAN INVENTED. The ingested package entity, the install-call uuid
/// and the `/namespace/<uuid>.json` segment must all agree; three places computing it three ways is
/// three chances to disagree with nothing able to notice, because each one is internally consistent.
///
/// The `waffler.package:` prefix is part of the input. A uuid computed over the bare fqid is a
/// different uuid that looks equally plausible.
pub fn package_uuid_for(fqid: &str) -> String {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, format!("waffler.package:{fqid}").as_bytes()).to_string()
}

#[cfg(test)]
#[path = "manifest_compiler.test.rs"]
mod tests;
