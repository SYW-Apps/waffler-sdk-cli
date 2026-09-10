//! `sdk_cli::project` — the developer's package project on disk.
//!
//! Spec: `sdk_cli::project`.
//!
//! WHAT A PROJECT IS: a source tree carrying `waffler.json`, an artifact to build, and optionally a
//! `namespace/` tree of entities and a UI plugin bundle. This subsystem turns that into the inputs
//! Publishing needs, and refuses early when it cannot.
//!
//! THE MANIFEST MODEL IS REPLACED, NOT MIGRATED, and the audit is why. The previous tool's manifest
//! and the format `waffler_core` installs share exactly one field NAME (`version`) and nothing else:
//! `namespace` became `fqid`, `module.runtime` became `hosting_mode`, `permissions` became
//! `permission_groups`, and artifacts gained content hashes that did not previously exist. A
//! field-by-field migration of a model with no overlapping fields is a rewrite wearing a
//! migration's clothes — so `waffler.json` is a new file with a new name, and a project carrying
//! only the old one is told so rather than misread.
//!
//! IT DOES NOT SIGN, UPLOAD, OR TALK TO A REGISTRY. That split is what lets the docker image build
//! a bundle with no network at all, and lets a publish of a pre-built bundle skip the project tree
//! entirely.

pub mod build_adapter;
pub mod manifest_compiler;
pub mod project_adapter;
pub mod project_orchestrator;
pub mod project_portal;
pub mod scaffold_specialist;
pub mod types;
