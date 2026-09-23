//! `sdk_cli::manifest-compiler` — what a developer AUTHORED becomes what a node INSTALLS.
//!
//! Spec: `sdk_cli::manifest-compiler` / `imanifest-compiler` / `manifest_compiler_impl`.
//!
//! PURE. No clock, no disk, no network — which is what makes every refusal testable from a literal
//! and every compilation testable without a build.
//!
//! EVERY REFUSAL HERE MIRRORS A RULE SOMETHING DOWNSTREAM ENFORCES — the registry at publish or the
//! node at install — AND THAT PARTY IS THE AUTHORITY. This side exists so a developer learns about a
//! malformed manifest in a second rather than after a release build and a hundred-megabyte upload.
//! Where the two disagree the downstream party is right and this is the bug, so a check is added here
//! only when something downstream already has it — never the reverse. A local check nothing
//! downstream enforces is a rule applied against honest developers and against nobody else.
//!
//! WHERE THE RULE IS CORE'S, CORE'S CODE IS CALLED, never copied: `MiddlewareScope::validate()` for a
//! middleware scope, `is_reserved_group_id` for a permission group id. A copy is a second chance for a
//! bundle to be packable and uninstallable.

use serde_json::json;

use super::types::{AuthoredPackage, LocatedArtifact, Violation};

/// The artifact kind that makes a package load in-process.
pub const KIND_DLL: &str = "Dll";
/// The artifact kind the frontend loads.
pub const KIND_UI_BUNDLE: &str = "UiBundle";

/// Advice about an authored manifest that does NOT stop a pack: shapes the node accepts that an author
/// probably does not mean.
///
/// NEVER A REFUSAL, AND NEVER A PASS. An empty list means "nothing to advise", not "sound" — soundness
/// is [`validate_authored`]'s answer — and a rule whose schema does not decode is skipped and left to the
/// node, which validates it.
///
/// THE BUS QUESTION IS ASKED OF THE MATCHER CORE'S FIREWALL IS PINNED TO, `bus_rule_covers`, never
/// re-derived. Asking it twice — for `register_middleware`, and for a command no rule names — tells a
/// rule scoped to that command from one admitting every command, without re-reading `verb` and `verbs`
/// here as a second spelling of the rule.
pub fn advise_authored(authored: &AuthoredPackage) -> Vec<super::types::Advisory> {
    use super::types::Advisory;
    use waffler_shared::{bus_rule_covers, FsVerb, PermissionEffect, RulePattern, RULE_KIND_BUS, TARGET_KIND_COMMAND};

    let declares_middleware = !authored.middleware.is_empty();
    let register = FsVerb("register_middleware".into());
    let nameless = FsVerb("a-command-no-rule-names".into());
    let mut advisories = Vec::new();

    for (g, group) in authored.permission_groups.iter().enumerate() {
        let Some(rules) = group.get("rules").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for (r, rule) in rules.iter().enumerate() {
            // CORE'S TYPES decide what a Bus Allow is. A rule that does not decode is the node's to refuse.
            let pattern = rule.get("pattern").and_then(|p| serde_json::from_value::<RulePattern>(p.clone()).ok());
            let effect = rule.get("effect").and_then(|e| serde_json::from_value::<PermissionEffect>(e.clone()).ok());
            let (Some(pattern), Some(PermissionEffect::Allow)) = (pattern, effect) else {
                continue;
            };
            if pattern.kind != RULE_KIND_BUS {
                continue;
            }
            let covers = |verb: &FsVerb| bus_rule_covers(&pattern, TARGET_KIND_COMMAND, "bus", Some(verb));
            if !covers(&register) {
                continue;
            }
            let field = format!("permissionGroups[{g}].rules[{r}]");
            if !covers(&nameless) {
                advisories.push(Advisory::new(
                    field,
                    "requests `bus:register_middleware`, which waffler_core 78b3acf8 and later grant through the host's own `host.middleware_grant`, approved with the operator's review of each `middleware[]` declaration, so on those nodes this rule admits no layer; keep it only if the package must also run on an older node",
                ));
            } else if declares_middleware {
                advisories.push(Advisory::new(
                    field,
                    "admits EVERY bus command, `register_middleware` among them; on waffler_core 78b3acf8 and later the host grants middleware registration itself, so scope this rule to the commands the package actually uses rather than dropping it, which would also remove whatever else it grants",
                ));
            }
        }
    }
    advisories
}

/// Check an authored manifest against the rules the registry and the node will apply.
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

    for (i, middleware) in authored.middleware.iter().enumerate() {
        let at = format!("middleware[{i}]");
        if middleware.id.trim().is_empty() {
            violations.push(Violation::new(format!("{at}.id"), "must not be empty"));
        }
        // THE ADDRESS THE INTERCEPTOR IS CALLED AT. A declaration with no handler registers, lists
        // and intercepts nothing — the inert-declaration shape — and the host has nothing to invoke.
        //
        // NOT CHECKED AGAINST `capabilities`: a package's BUS-served capabilities are served by its
        // handler and never appear in that list, so cross-referencing would refuse the ordinary
        // case. A rule that fails the correct shape is worse than no rule.
        if middleware.handler.trim().is_empty() {
            violations.push(Violation::new(
                format!("{at}.handler"),
                "must name the capability in this package that the host invokes for each matched envelope; without one the declaration registers and intercepts nothing",
            ));
        }
        // CORE'S OWN RULE, CALLED — never reimplemented. One computation for three consumers: the
        // node refuses registration on it, this tool refuses a pack on it, and a review surface
        // describes the scope from the same answer. Two copies would be two chances for a package to
        // be packable and unregisterable.
        //
        // IT IS WHAT REPLACED A FREE-FORM BAG THAT PRODUCED FOUR DEFECTS AT ONCE — a key meaning two
        // things to two matchers, a dimension one honoured and the other ignored, one globbing where
        // the other used `==`, and a disable that disabled on one chain only. A bag has no arity, so
        // nothing could disagree loudly. Re-deriving its replacement here would rebuild the cause.
        if let Err(reason) = middleware.scope.validate() {
            violations.push(Violation::new(format!("{at}.scope"), reason));
        }
    }

    // THE HOST-OWNED GROUP NAMESPACE. Core synthesizes groups under `host.` from operator decisions and
    // approves them itself — the first carries a middleware package's registration grant — so an author
    // writing one is writing the host's rule, and one written before the host defines an id squats it.
    //
    // CORE'S PREDICATE, CALLED: the same function refuses the install, so a bundle refused here is one
    // the node would refuse, and nothing more. Only the id is read; the group's schema is still
    // security's, and still passed through unmodelled.
    for (i, group) in authored.permission_groups.iter().enumerate() {
        if let Some(id) = group.get("id").and_then(serde_json::Value::as_str) {
            if waffler_shared::is_reserved_group_id(id) {
                violations.push(Violation::new(
                    format!("permissionGroups[{i}].id"),
                    format!(
                        "'{id}' is in the reserved '{}' namespace, which the host owns (its middleware-approval grant group is '{}'); an author may not declare a group there, and the node refuses the install",
                        waffler_shared::HOST_GROUP_ID_PREFIX,
                        waffler_shared::MIDDLEWARE_GRANT_GROUP_ID
                    ),
                ));
            }
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

    // WRITTEN FROM THE AUTHORED LIST, which until now was a literal `[]`. Core's consumer side was
    // complete the whole time — the supervisor walks these, owner-tags each and registers it on the
    // per-package chain and the global bus chain — so the only thing missing was a producer.
    //
    // MAPPED FIELD BY FIELD, LIKE EVERY OTHER DECLARATION IN THIS BODY, and the first attempt did
    // not do that. Serializing the authored type directly looked equivalent and was not: the
    // authored model is `camelCase` BECAUSE A HUMAN WRITES `waffler.json`, while `/.manifest` is
    // core's own shape and `MiddlewareDeclaration` reads `needs_payload` / `needs_headers`. The
    // bundle decoded as `missing field 'needs_payload'` — a manifest this tool wrote and core
    // cannot read.
    //
    // THE TWO SPELLINGS ARE THE POINT, not an accident to be smoothed over. A developer editing
    // JSON should not have to know which language read it, and core should not have to accept a
    // second spelling of its own type. This function is where the two meet, which is why every
    // other field here is written out rather than derived.
    //
    // AN ABSENT OPTIONAL STAYS ABSENT rather than becoming null: `target` absent means "intercept
    // every routed call", and `target: null` is a different claim about a field core reads.
    let middleware: Vec<serde_json::Value> = authored
        .middleware
        .iter()
        .map(|m| {
            let mut entry = serde_json::Map::new();
            entry.insert("id".into(), json!(m.id));
            entry.insert("handler".into(), json!(m.handler));
            entry.insert("needs_payload".into(), json!(m.needs_payload));
            entry.insert("needs_headers".into(), json!(m.needs_headers));
            // THE SCOPE IS SERIALIZED BY CORE'S OWN TYPE, which is the point of embedding it: there
            // is no name mapping here to lose a field the way `needs_payload` was lost.
            entry.insert(
                "scope".into(),
                serde_json::to_value(&m.scope).expect("a middleware scope serializes"),
            );
            // WRITTEN EVEN AT THEIR DEFAULTS, for the reason a dependency's `optional` flag is: a
            // manifest from this tool must be tellable apart from one that predates the fields. `kind`
            // goes through core's own type, so its spelling cannot drift from the node's.
            entry.insert("required".into(), json!(m.required));
            entry.insert("kind".into(), serde_json::to_value(m.kind).expect("a middleware kind serializes"));
            if let Some(priority) = m.priority {
                entry.insert("priority".into(), json!(priority));
            }
            serde_json::Value::Object(entry)
        })
        .collect();

    let fast_lane_requests: Vec<serde_json::Value> = authored
        .fast_lane_requests
        .iter()
        // The reviewable ASK, and NOTHING ABOUT ITS REVIEW. `review` used to be written as
        // "Pending" here, which is a package stating a verdict about itself inside a signed bundle.
        // Core ignores it for authorization -- a lane is granted from the operator's decisions and
        // never from the manifest, so a bundle claiming "Approved" gets nothing -- but the field is
        // stored on the record and served by the catalog, where a surface that renders it shows the
        // manifest's opinion as if it were the node's. That is the same shape as
        // `permission_groups[].status`, which over-reported 5x on the beta. Omitted, it
        // serde-defaults to Pending at every reader, which is the one true answer for a bundle.
        .map(|r| json!({ "target": r.target, "secure": r.secure, "required": true }))
        .collect();

    let mut body = json!({
        "fqid": authored.fqid,
        "version": authored.version,
        "hosting_mode": hosting_mode,
        "permission_groups": authored.permission_groups,
        "dependencies": dependencies,
        "capabilities": capabilities,
        "ui_plugins": ui_plugins,
        "middleware": middleware,
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
