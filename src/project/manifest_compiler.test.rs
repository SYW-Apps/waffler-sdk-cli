#![allow(non_snake_case)]
// Emphatic capitals in a test name are this codebase convention: a name that says WHAT IS
// BEING PROVEN reads better than one that obeys a lint, and a warning nobody clears becomes a
// warning nobody reads - which is how a real one gets missed.

//! Tests for the manifest compiler.
//!
//! EVERY REFUSAL IS ASSERTED BESIDE ITS POSITIVE CASE. "Refuse when wrong" can pass for a check whose
//! condition is inverted or over-broad, and the failure mode is a tool that refuses valid manifests —
//! which reads to a developer as the tool being broken rather than their file. So each test that proves
//! something is rejected also proves the neighbouring valid shape is accepted.

use super::*;
use crate::project::types::*;

/// A manifest that should pass every check, as the baseline every negative test mutates.
fn sound() -> AuthoredPackage {
    AuthoredPackage {
        fqid: "syw.probe.echo".into(),
        version: "1.0.0".into(),
        description: "a fixture".into(),
        core_compatibility: Some("^0.1".into()),
        artifacts: vec![DeclaredArtifact {
            path: Some("target/release/libecho.so".into()),
            from_build: false,
            kind: "Dll".into(),
            entry_point: Some("wf_init".into()),
            name: None,
        }],
        dependencies: vec![DeclaredDependency { fqid: "syw.system.store".into(), version: "^1.0".into() }],
        capabilities: vec![],
        permission_groups: vec![],
        fast_lane_requests: vec![],
        ui_plugins: vec![],
        build: Some(BuildDeclaration { manifest_path: "Cargo.toml".into() }),
    }
}

fn fields(violations: &[Violation]) -> Vec<&str> {
    violations.iter().map(|v| v.field.as_str()).collect()
}

#[test]
fn a_sound_manifest_has_no_violations() {
    // THE ANCHOR FOR EVERY NEGATIVE TEST BELOW. Without it, a check that refused everything would make
    // all of them pass.
    assert_eq!(validate_authored(&sound()), vec![], "the baseline must be accepted or every refusal test below proves nothing");
}

#[test]
fn an_undotted_fqid_is_refused_and_a_dotted_one_is_not() {
    let mut m = sound();
    m.fqid = "echo".into();
    assert_eq!(fields(&validate_authored(&m)), vec!["fqid"]);

    m.fqid = "syw.echo".into();
    assert!(validate_authored(&m).is_empty(), "two segments is a fully-qualified id");
}

#[test]
fn a_version_that_is_not_semver_is_refused() {
    let mut m = sound();
    m.version = "1.0".into();
    assert_eq!(fields(&validate_authored(&m)), vec!["version"]);
}

#[test]
fn core_compatibility_must_be_a_REQUIREMENT_and_absence_is_allowed() {
    let mut m = sound();
    // A version where a range belongs still PARSES as a range in semver's grammar, so the interesting
    // negative is a string that is neither.
    m.core_compatibility = Some("not-a-range".into());
    assert_eq!(fields(&validate_authored(&m)), vec!["coreCompatibility"]);

    // ABSENT IS PERMITTED, and that is a real decision rather than a gap: a bundle declaring nothing
    // installs UNCHECKED behind a warning, and refusing it here would make this tool unable to pack
    // packages core is willing to install.
    m.core_compatibility = None;
    assert!(validate_authored(&m).is_empty());

    m.core_compatibility = Some("^0.1".into());
    assert!(validate_authored(&m).is_empty());
}

#[test]
fn a_dll_without_an_entry_point_is_refused_but_a_ui_bundle_needs_none() {
    let mut m = sound();
    m.artifacts[0].entry_point = None;
    assert_eq!(fields(&validate_authored(&m)), vec!["artifacts[0].entryPoint"]);

    // THE ASYMMETRY IS THE POINT. A UiBundle is loaded by the frontend and no process host calls a
    // symbol in it, so requiring one would refuse every UI-only package.
    m.artifacts[0].kind = "UiBundle".into();
    assert!(validate_authored(&m).is_empty(), "a UiBundle has no entry point to declare");
}

#[test]
fn an_unknown_artifact_kind_is_refused() {
    let mut m = sound();
    m.artifacts[0].kind = "Wasm".into();
    let v = validate_authored(&m);
    assert!(v.iter().any(|v| v.field == "artifacts[0].kind"), "got {v:?}");
}

#[test]
fn a_path_that_escapes_the_project_is_refused_in_every_spelling() {
    for escaping in ["../elsewhere/libecho.so", "/usr/lib/libecho.so", "a/../../b/libecho.so"] {
        let mut m = sound();
        m.artifacts[0].path = Some(escaping.into());
        let v = validate_authored(&m);
        assert!(
            v.iter().any(|v| v.field == "artifacts[0].path"),
            "'{escaping}' should be refused: a bundle assembled from files outside the project is one \
             nobody can reproduce from the repository. got {v:?}"
        );
    }
    // The neighbouring valid shape: a nested relative path is fine.
    let mut m = sound();
    m.artifacts[0].path = Some("build/out/libecho.so".into());
    assert!(validate_authored(&m).is_empty());
}

#[test]
fn a_capability_naming_no_declared_artifact_is_refused_in_both_directions() {
    let mut m = sound();
    m.capabilities.push(DeclaredCapability {
        fqid: "sim.echo".into(),
        kind: "RuntimePrimitive".into(),
        target_host: "runtime-wack".into(),
        artifact: "libnothing.so".into(),
        symbol: Some("sim_echo".into()),
        inputs: vec![],
    });
    assert_eq!(fields(&validate_authored(&m)), vec!["capabilities[0].artifact"]);

    // Named correctly, it passes — which is what proves the check reads the NAME and not merely the
    // presence of a capability.
    m.capabilities[0].artifact = "libecho.so".into();
    assert!(validate_authored(&m).is_empty());
}

#[test]
fn a_runtime_primitive_must_name_its_symbol() {
    let mut m = sound();
    m.capabilities.push(DeclaredCapability {
        fqid: "sim.echo".into(),
        kind: "RuntimePrimitive".into(),
        target_host: "runtime-wack".into(),
        artifact: "libecho.so".into(),
        symbol: None,
        inputs: vec![],
    });
    assert_eq!(fields(&validate_authored(&m)), vec!["capabilities[0].symbol"]);
}

#[test]
fn a_ui_plugin_must_name_a_UiBundle_and_not_merely_an_artifact() {
    let mut m = sound();
    m.ui_plugins.push(DeclaredUiPlugin {
        id: "marketplace".into(),
        slot: String::new(),
        // Points at the Dll, which exists. THE KIND IS WHAT MAKES THIS WRONG, and a check that only
        // tested existence would accept it.
        artifact: "libecho.so".into(),
    });
    assert_eq!(fields(&validate_authored(&m)), vec!["uiPlugins[0].artifact"]);

    m.artifacts.push(DeclaredArtifact {
        path: Some(".ui/marketplace.js".into()),
        from_build: false,
        kind: "UiBundle".into(),
        entry_point: None,
        name: None,
    });
    m.ui_plugins[0].artifact = "marketplace.js".into();
    assert!(validate_authored(&m).is_empty());
}

#[test]
fn an_empty_slot_is_accepted_because_it_is_the_honest_value_for_a_plugin_that_mounts_nowhere() {
    let mut m = sound();
    m.artifacts.push(DeclaredArtifact { path: Some(".ui/p.js".into()), from_build: false, kind: "UiBundle".into(), entry_point: None, name: None });
    m.ui_plugins.push(DeclaredUiPlugin { id: "p".into(), slot: String::new(), artifact: "p.js".into() });
    assert!(validate_authored(&m).is_empty());

    // AND AN INVENTED SLOT NAME IS *NOT* REFUSED, deliberately. A packer has no business knowing the
    // frontend's regions, and coupling this to that list would make every new region a change here.
    // Asserted so the absence of a check is a decision on the record rather than something a later
    // reader adds "for completeness".
    m.ui_plugins[0].slot = "not.a.real.region".into();
    assert!(validate_authored(&m).is_empty(), "the slot name is the frontend's business, not the packer's");
}

#[test]
fn a_dependency_version_must_be_a_requirement() {
    let mut m = sound();
    m.dependencies[0].version = "not-a-range".into();
    assert_eq!(fields(&validate_authored(&m)), vec!["dependencies[0].version"]);
}

#[test]
fn every_violation_is_reported_not_just_the_first() {
    let mut m = sound();
    m.fqid = "echo".into();
    m.version = "nope".into();
    m.artifacts[0].entry_point = None;
    let v = validate_authored(&m);
    // FIXING A MANIFEST ONE REFUSED FIELD PER RUN is how a five-minute correction becomes an afternoon.
    assert_eq!(v.len(), 3, "expected all three, got {v:?}");
}

#[test]
fn hosting_mode_is_DERIVED_from_what_was_built() {
    let m = sound();
    let dll = LocatedArtifact {
        name: "libecho.so".into(),
        kind: "Dll".into(),
        entry_point: Some("wf_init".into()),
        absolute_path: "/tmp/libecho.so".into(),
        size_bytes: 10,
    };
    assert_eq!(compile_manifest_body(&m, &[dll.clone()])["hosting_mode"], "DIRECT");

    // A package shipping only a UI bundle stays HOSTED and starts nothing, because a UI bundle is
    // loaded by the frontend and not by a process host.
    let ui = LocatedArtifact { kind: "UiBundle".into(), entry_point: None, ..dll.clone() };
    assert_eq!(compile_manifest_body(&m, &[ui])["hosting_mode"], "HOSTED");

    // And with nothing built at all.
    assert_eq!(compile_manifest_body(&m, &[])["hosting_mode"], "HOSTED");
}

#[test]
fn runtime_managed_fields_are_ABSENT_rather_than_emitted_empty() {
    let body = compile_manifest_body(&sound(), &[]);
    // core's own warnings depend on telling "declared nothing" from "declared an empty value" apart, so
    // an absent field must stay absent.
    for runtime_managed in ["identity", "approved_group_ids", "fast_lane_grants"] {
        assert!(
            body.get(runtime_managed).is_none(),
            "{runtime_managed} is minted by a node; emitting it here would make a bundle that declares \
             nothing indistinguishable from one whose author wrote a blank"
        );
    }
}

#[test]
fn the_artifacts_array_is_left_for_the_writer() {
    let body = compile_manifest_body(&sound(), &[]);
    // Its hashes are facts about bytes that do not exist yet. A hash computed here and copied there is
    // a claim a later step could contradict.
    assert!(body.get("artifacts").is_none(), "hashes are measured over the bytes actually written");
}

#[test]
fn core_compatibility_is_emitted_only_when_declared() {
    let mut m = sound();
    assert_eq!(compile_manifest_body(&m, &[])["core_compatibility"], "^0.1");

    m.core_compatibility = None;
    assert!(
        compile_manifest_body(&m, &[]).get("core_compatibility").is_none(),
        "absent must stay absent so core's warning tells the truth about which case it is looking at"
    );

    // A blank string is treated as undeclared, because a bundle whose author wrote nothing meaningful
    // and one that declared nothing are the same situation, and core's warning should fire for both.
    m.core_compatibility = Some("   ".into());
    assert!(compile_manifest_body(&m, &[]).get("core_compatibility").is_none());
}

#[test]
fn the_package_uuid_is_the_runtimes_formula_including_its_prefix() {
    // MEASURED AGAINST A KNOWN VALUE rather than against a second call to the same function, which
    // would only prove determinism. The prefix is part of the input: a uuid over the bare fqid is a
    // different uuid that looks equally plausible, and the three places that must agree are the
    // ingested entity, the install-call uuid and the namespace segment.
    let with_prefix = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, b"waffler.package:syw.probe.echo").to_string();
    let without = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, b"syw.probe.echo").to_string();
    assert_eq!(package_uuid_for("syw.probe.echo"), with_prefix);
    assert_ne!(with_prefix, without, "the prefix must change the answer, or this test proves nothing");
}

#[test]
fn a_capability_with_no_declared_inputs_carries_an_empty_list_meaning_UNDECLARED() {
    let mut m = sound();
    m.capabilities.push(DeclaredCapability {
        fqid: "sim.echo".into(),
        kind: "RuntimePrimitive".into(),
        target_host: "runtime-wack".into(),
        artifact: "libecho.so".into(),
        symbol: Some("sim_echo".into()),
        inputs: vec![],
    });
    let dll = LocatedArtifact {
        name: "libecho.so".into(),
        kind: "Dll".into(),
        entry_point: Some("wf_init".into()),
        absolute_path: "/tmp/libecho.so".into(),
        size_bytes: 10,
    };
    let body = compile_manifest_body(&m, &[dll]);
    assert_eq!(body["capabilities"][0]["inputs"], serde_json::json!([]));
    // The descriptor carries the symbol and the ffi signature the runtime binds against.
    assert_eq!(body["capabilities"][0]["metadata"]["node_descriptor"]["c_symbol"], "sim_echo");
    assert_eq!(body["capabilities"][0]["metadata"]["node_descriptor"]["ffi_signature"], "wf_prim_v1");
}

#[test]
fn declared_inputs_keep_their_ORDER_because_the_order_is_the_contract() {
    let mut m = sound();
    m.capabilities.push(DeclaredCapability {
        fqid: "sim.fetch".into(),
        kind: "RuntimePrimitive".into(),
        target_host: "runtime-wack".into(),
        artifact: "libecho.so".into(),
        symbol: Some("sim_fetch".into()),
        inputs: vec![
            DeclaredParameter { key: "method".into(), r#type: Some("String".into()) },
            DeclaredParameter { key: "url".into(), r#type: None },
        ],
    });
    let dll = LocatedArtifact {
        name: "libecho.so".into(),
        kind: "Dll".into(),
        entry_point: Some("wf_init".into()),
        absolute_path: "/tmp/libecho.so".into(),
        size_bytes: 10,
    };
    let body = compile_manifest_body(&m, &[dll]);
    let inputs = &body["capabilities"][0]["inputs"];
    // THE POSITIONS, NOT MERELY THE PRESENCE. Sorting them alphabetically would put `method` before
    // `url` too, so the test uses a pair whose declared order differs from its sorted order.
    assert_eq!(inputs[0]["key"], "method");
    assert_eq!(inputs[1]["key"], "url");
    // An untyped parameter is Dynamic, which is accepted and defers every type error to runtime.
    assert_eq!(inputs[1]["type"]["kind"], "Dynamic");
    assert_eq!(inputs[0]["type"]["kind"], "String");
}

#[test]
fn a_fast_lane_request_is_a_PENDING_REQUEST_never_a_grant() {
    let mut m = sound();
    m.fast_lane_requests.push(FastLaneRequest { target: "vault".into(), secure: true });
    let body = compile_manifest_body(&m, &[]);
    let request = &body["fast_lane_requests"][0];
    assert_eq!(request["target"], "vault");
    assert_eq!(request["secure"], true);
    // The reviewable ASK. A grant is what an approval produces on a node, and a bundle that could
    // declare one would be a bundle that grants itself a fast lane.
    assert_eq!(request["review"], "Pending");
    assert_eq!(request["required"], true);
    assert!(body.get("fast_lane_grants").is_none(), "a bundle must not be able to declare a grant");
}


#[test]
fn an_artifact_must_name_EXACTLY_ONE_authority_for_where_it_is() {
    // `path` says the developer knows where the file is; `fromBuild` says the build tool does. Both
    // together is a manifest that could disagree with itself, and neither is one that names nothing.
    let mut m = sound();
    m.artifacts[0].from_build = true;
    assert_eq!(fields(&validate_authored(&m)), vec!["artifacts[0]"], "path AND fromBuild must be refused");

    m.artifacts[0].path = None;
    assert!(validate_authored(&m).is_empty(), "fromBuild alone is the ordinary case");

    m.artifacts[0].from_build = false;
    assert_eq!(fields(&validate_authored(&m)), vec!["artifacts[0]"], "neither must be refused");
}

#[test]
fn a_from_build_artifact_needs_an_explicit_name_to_be_referenceable() {
    // WHAT THE BUILD TOOL WILL CALL IT IS NOT KNOWABLE AT VALIDATION TIME — that is the whole reason
    // `fromBuild` exists. So a capability cannot reference such an artifact unless the manifest also
    // gives it a name, and the refusal says which names ARE available.
    let mut m = sound();
    m.artifacts[0].path = None;
    m.artifacts[0].from_build = true;
    m.capabilities.push(DeclaredCapability {
        fqid: "sim.echo".into(),
        kind: "RuntimePrimitive".into(),
        target_host: "runtime-wack".into(),
        artifact: "libecho.so".into(),
        symbol: Some("sim_echo".into()),
        inputs: vec![],
    });
    let v = validate_authored(&m);
    assert!(v.iter().any(|v| v.field == "capabilities[0].artifact"), "got {v:?}");

    // Give it a name and the reference resolves. This is the positive half: without it, a check that
    // refused every fromBuild capability reference would pass the assertion above.
    m.artifacts[0].name = Some("libecho.so".into());
    assert!(validate_authored(&m).is_empty(), "an explicit name makes it referenceable");
}

#[test]
fn a_bundle_name_comes_from_the_resolved_file_and_an_explicit_name_wins() {
    let mut a = DeclaredArtifact {
        path: Some("target/release/libecho.so".into()),
        from_build: false,
        kind: "Dll".into(),
        entry_point: Some("wf_init".into()),
        name: None,
    };
    // THE RESOLVED FILE'S OWN NAME. A `fromBuild` artifact has no declared path to take a name from,
    // and for a declared one the two are identical — so taking it from the file that will actually be
    // embedded is the only spelling that works for both.
    let resolved = std::path::Path::new("/cache/target/release/libsyw_example_hello.so");
    assert_eq!(a.bundle_name(resolved), "libsyw_example_hello.so");

    a.name = Some("echo.so".into());
    assert_eq!(a.bundle_name(resolved), "echo.so", "an explicit name overrides the file's");

    // An empty explicit name falls back rather than producing an artifact with no name — a zip entry
    // called `artifact/` is a directory, not a file.
    a.name = Some(String::new());
    assert_eq!(a.bundle_name(resolved), "libsyw_example_hello.so");
}
