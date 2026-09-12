//! The project subsystem's data model.
//!
//! Spec: `sdk_cli::project` types — `authored-package`, `declared-artifact`, `declared-dependency`,
//! `declared-capability`, `declared-parameter`, `declared-ui-plugin`, `located-artifact`,
//! `bundle-plan`, `build-report`.
//!
//! ## THE AUTHORED MODEL CARRIES ONLY WHAT A DEVELOPER MAY WRITE
//!
//! `waffler_shared::InstalledPackage` — the shape core decodes from a bundle's `/.manifest` — mixes
//! three populations that a developer must not be asked to tell apart:
//!
//!   * DECLARED — fqid, version, dependencies, capabilities, permission groups, fast-lane
//!     requests, ui plugins, core compatibility;
//!   * DERIVED — the `artifacts` array, whose content hashes are facts about bytes that do not
//!     exist until a build has run;
//!   * RUNTIME-MANAGED — `identity`, `enabled`, `approved_group_ids`, `fast_lane_grants`, minted by
//!     a node and meaningless in a bundle.
//!
//! An authoring manifest exposing all three would invite a developer to write an identity and a
//! hash, and both would be silently overwritten or, worse, believed. So only the first population
//! appears here. A field a developer cannot write is a field that cannot be wrong.

use serde::{Deserialize, Serialize};

/// The name of the authoring manifest. NOT `package.json`, and the difference is the safety.
///
/// The legacy tool's `package.json` model shares exactly one field NAME with the format core
/// installs (`version`) and no meaning: `namespace` became `fqid`, `module.runtime` became
/// `hosting_mode`, `permissions` became `permission_groups`, and artifacts gained content hashes
/// that did not previously exist. Read as this model, such a file PARSES — every field it does not
/// carry is defaulted — and produces a bundle that is wrong in every field. A distinct filename
/// turns that silent misread into "this project has no waffler.json", which is a sentence a
/// developer can act on.
pub const MANIFEST_FILE: &str = "waffler.json";

/// The name of the file whose presence means a project predates this model.
pub const LEGACY_MANIFEST_FILE: &str = "package.json";

/// `waffler.json` — everything a developer writes about their package.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct AuthoredPackage {
    /// The fully-qualified package id, e.g. `syw.probe.echo`. It is the registry namespace, the
    /// install key, and the seed of the deterministic package uuid — one string doing three jobs.
    pub fqid: String,
    /// SemVer.
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// The core CONTRACT range this package is built against, e.g. `^0.1`.
    ///
    /// ABSENT MEANS THE BUNDLE DECLARED NOTHING and installs UNCHECKED behind a warning — which is
    /// not the same as compatible. Optional here only so an existing project keeps packing; a
    /// scaffold always writes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_compatibility: Option<String>,
    /// The files to build and embed. Empty means the package ships no loadable code and installs
    /// HOSTED.
    #[serde(default)]
    pub artifacts: Vec<DeclaredArtifact>,
    #[serde(default)]
    pub dependencies: Vec<DeclaredDependency>,
    #[serde(default)]
    pub capabilities: Vec<DeclaredCapability>,
    /// Passed through as authored. The schema belongs to security, and re-modelling it here would
    /// create a second definition that drifts from the one the registry validates.
    #[serde(default)]
    pub permission_groups: Vec<serde_json::Value>,
    /// Requested fast lanes. The reviewable ASK; a GRANT is what an approval produces on a node and
    /// can never be declared here.
    #[serde(default)]
    pub fast_lane_requests: Vec<FastLaneRequest>,
    #[serde(default)]
    pub ui_plugins: Vec<DeclaredUiPlugin>,
    /// Bus middleware this package contributes — interceptors core runs around routed calls.
    ///
    /// EMPTY IS THE ORDINARY CASE AND WAS UNTIL NOW THE ONLY ONE: `compile_manifest_body` wrote
    /// `"middleware": []` as a literal, so no bundle could declare an interceptor whatever its
    /// author wrote.
    ///
    /// DECLARING ONE COMMITS THE PACKAGE TO A GRANT IT MUST ALSO REQUEST. Core registers each
    /// declaration on the global bus chain, that registration is gated on `bus:register_middleware`,
    /// and a failure there is FATAL rather than skipped — an interceptor that silently did not
    /// register is an auth package checking nothing, which is the worst outcome available. So a
    /// package declaring middleware without a permission group asking for that grant does not run.
    #[serde(default)]
    pub middleware: Vec<DeclaredMiddleware>,
    /// The crate manifest to build, project-relative. Absent means nothing is built and the
    /// declared artifacts are expected to exist already.
    ///
    /// NAMED, NEVER INFERRED. Deriving a cargo package name from the package fqid worked until the
    /// two differed, and then it built the wrong crate and packed its artifact under the right
    /// name — a bundle that installs and is not the software anyone wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildDeclaration>,
}

/// What to build, and with what.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct BuildDeclaration {
    /// Path to the crate manifest, relative to the project directory.
    pub manifest_path: String,
}

/// One file a project builds and embeds, as the DEVELOPER declares it.
///
/// NO HASH, AND THE ABSENCE IS THE DESIGN. A hash a developer writes is a claim about bytes that do
/// not exist yet; a hash computed over the bytes actually being written is a fact. The two
/// disagreeing is precisely what a content hash exists to detect, so only one of them may be
/// authorable — and it is neither.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct DeclaredArtifact {
    /// Project-relative path to a file THIS TOOL DOES NOT BUILD — a UI bundle from vite, a data file,
    /// anything produced by other tooling. Resolved exactly, never searched.
    ///
    /// Exactly one of `path` and `from_build` must be given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Take this artifact from the declared build's OUTPUT, wherever the toolchain put it.
    ///
    /// ## WHY THIS EXISTS RATHER THAN A LITERAL PATH
    ///
    /// A manifest declaring `target/release/libfoo.so` encodes two assumptions that belong to the
    /// build tool and not to the developer, and both were wrong the first time this was run for real:
    ///
    ///   * WHERE the output goes. `CARGO_TARGET_DIR` — which our own container build sets to a cache
    ///     mount — `build.target-dir` in a config file, and a workspace's shared target directory all
    ///     move it. The pack failed with "declared artifact does not exist", naming a path nobody had
    ///     typed.
    ///   * WHAT the file is called. The toolchain emits `libfoo.so`, `foo.dll` or `libfoo.dylib`
    ///     depending on the platform, so a literal path makes the manifest single-platform — and a
    ///     bundle packed on Windows carries a module no Linux node can load.
    ///
    /// Asking the toolchain removes both. IT IS NOT A SEARCH: the build tool is the authority on where
    /// its own output went, and one authoritative answer replaces a list of guesses.
    #[serde(default)]
    pub from_build: bool,
    /// `Dll` for a loadable module, `UiBundle` for a frontend bundle.
    pub kind: String,
    /// The symbol a host calls to initialize a Dll — `wf_init` for the Rust SDK. Absent for a
    /// UiBundle, which no process host loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<String>,
    /// The name the artifact takes inside the bundle. Defaults to the file name of `path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl DeclaredArtifact {
    /// The name this artifact takes inside the bundle.
    ///
    /// ONE DERIVATION, USED EVERYWHERE. The cross-reference check, the manifest's artifact refs and
    /// the zip entry path all resolve a declaration to a name; three implementations of that would
    /// be three chances for a capability to name an artifact that the writer files under something
    /// else, and the bundle would be internally inconsistent with nothing able to say so.
    pub fn bundle_name(&self, resolved: &std::path::Path) -> String {
        if let Some(name) = self.name.as_deref().filter(|n| !n.is_empty()) {
            return name.to_string();
        }
        // THE RESOLVED FILE'S OWN NAME, not the declaration's last segment. A `from_build` artifact has
        // no declared path to take a name from, and for a declared one the two are identical — so
        // taking it from the file that will actually be embedded is both correct and the only spelling
        // that works for both.
        resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// A package this package needs: an fqid and a version REQUIREMENT.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct DeclaredDependency {
    pub fqid: String,
    /// A SemVer requirement the registry resolves, e.g. `^1.0`. A RANGE, not a pin — a pin makes
    /// every upstream patch a republish here.
    pub version: String,
    /// Whether this package can run WITHOUT the dependency. Absent means REQUIRED.
    ///
    /// CORE HAS SUPPORTED THIS SINCE `CrateDependency` GAINED THE FLAG, AND THIS TOOL COULD NOT
    /// EXPRESS IT. Every dependency authored here was stamped required, silently, with no field to
    /// write and no error to say why — so an author who wanted an optional dependency had no way to
    /// ask and no way to find out they had not.
    ///
    /// DEFAULTS TO REQUIRED, matching core's own default, and that is the fail-safe direction: a
    /// manifest written before this field existed keeps the meaning it had, and the failure mode of
    /// a forgotten flag is a package that refuses to start rather than one that starts broken.
    #[serde(default)]
    pub optional: bool,
}

/// A bus middleware this package contributes: an interceptor core runs around routed calls.
///
/// THIS TOOL COULD NOT AUTHOR ONE. `compile_manifest_body` wrote `"middleware": []` as a literal and
/// the authored model had no field — the third instance of that hole after `dependencies` and
/// `ui_plugins`, while core's consumer side was complete the whole time.
///
/// ## THE SCOPE IS CORE'S OWN TYPE, EMBEDDED, NOT RE-MODELLED HERE
///
/// A free-form filter bag produced FOUR defects in core at once: a key that was an owner tag to one
/// matcher and a source filter to another; a dimension one matcher honoured and the other ignored
/// (so a declaration naming one service intercepted every message on the node); one side globbing
/// where the other compared with `==`; and a disable that disabled on one chain only. **A free-form
/// bag has no arity, so nothing can disagree loudly.**
///
/// Re-modelling the typed replacement here would rebuild exactly that — two definitions of one
/// matcher, free to drift, with the authoring side never the one that runs. So the scope's fields
/// are `snake_case` while the rest of this file is `camelCase`, deliberately. That visible seam is
/// the cheaper cost: the alternative is a mapping layer between two shapes, and a mapping layer is
/// what silently dropped `needs_payload` from this tool's own output one change earlier.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE — except inside `scope` and `kind`,
// whose contents are core's types and keep core's spelling.
//
// AN UNKNOWN KEY IS REFUSED BY NAME. On a declaration that decides what an interceptor sees, a key that
// silently does nothing changes behaviour without a word: `needsHeader` for `needsHeaders` hands an
// auth layer no bearer token, and an author-written `owner`, `ownerFqid` or `consentDigest` looks like
// identity or consent the package gave itself — the node overwrites all three. The retired `filters`
// bag and `target` fall under the same refusal. The scope types already refuse unknown keys this way.
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredMiddleware {
    /// Unique within the package. Re-registering the same id REPLACES the earlier declaration.
    pub id: String,
    /// The capability in THIS package's own module that the host invokes for each matched envelope.
    ///
    /// IT IS THE ADDRESS THE INTERCEPTOR IS CALLED AT. The legacy ABI passed it directly —
    /// `host_register_middleware(cap_ptr, cap_len)` — and the migrated declaration had lost it.
    ///
    /// NOT NAMED `capability`: the scope's `capabilities` dimension already means the INTERCEPTED
    /// capability — which call is being made.
    ///
    /// NOT CROSS-CHECKED AGAINST `capabilities`. A package's BUS-served capabilities are served by
    /// its handler and never appear in that list, so a check against it would refuse the ordinary
    /// case — and a rule that fails the correct shape is worse than no rule.
    pub handler: String,
    /// What this interceptor sees. CORE'S TYPE, so there is exactly one definition of the matcher.
    ///
    /// COMMANDS AND EVENTS ARE SEPARATE BLOCKS, which is a security property made structural:
    /// knowing which events a service listens to is enough to manipulate that service, so event
    /// interception can never be acquired by leaving a topic list empty inside a command block.
    ///
    /// NOTHING IS ACQUIRED BY OMISSION — a block that narrows nothing and does not say
    /// `everything: true` is refused, here and again at the node. The broadest scope can no longer
    /// be the emptiest-looking declaration, because the emptiest-looking declaration does not
    /// register.
    pub scope: waffler_shared::MiddlewareScope,
    /// Whether the interceptor is handed the call's payload.
    ///
    /// ASK FOR NOTHING THAT IS NOT READ: a payload the interceptor does not inspect is bytes
    /// crossing a package boundary on every matched call.
    #[serde(default)]
    pub needs_payload: bool,
    /// Whether the interceptor is handed the call's headers — where a bearer token travels.
    #[serde(default)]
    pub needs_headers: bool,
    /// Where this sits in the assembled chain relative to OTHER packages' interceptors. Absent takes
    /// core's default. It orders across packages, so a number chosen against one node's package set
    /// changes meaning when another package is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
    /// Can the PACKAGE run without this interceptor? Absent means REQUIRED — core's default, and the
    /// fail-safe reading of a declaration nobody considered.
    ///
    /// IT DECIDES THE COST OF A DENIAL, NOT THE SCOPE OF CONSENT: denied or unreviewed and required
    /// refuses the package's enable, optional skips the layer and reports it. Core keeps it out of the
    /// consent digest for exactly that reason.
    #[serde(default = "required_unless_said_otherwise")]
    pub required: bool,
    /// May this interceptor's VERDICT stop or alter a message? CORE'S ENUM — `Enforcing` (the default)
    /// or `Observing` — embedded as the scope is, so a misspelling is a parse error rather than an
    /// audit layer an operator believes only watches. Folded into the consent digest with the scope,
    /// so moving a declaration from Observing to Enforcing re-presents it for review.
    #[serde(default)]
    pub kind: waffler_shared::MiddlewareKind,
}

/// A middleware declaration that says nothing about `required` is REQUIRED, matching core's default.
fn required_unless_said_otherwise() -> bool {
    true
}

/// A capability this package contributes to a host.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct DeclaredCapability {
    /// The capability's own fully-qualified id — distinct from the package's, since one package may
    /// contribute several.
    pub fqid: String,
    /// `RuntimePrimitive` for a blueprint-callable node; other kinds target other hosts.
    pub kind: String,
    /// Which host serves it, e.g. `runtime-wack`.
    pub target_host: String,
    /// The declared artifact `name` that realizes this capability.
    pub artifact: String,
    /// The C symbol the runtime binds, for a RuntimePrimitive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// The calling contract, IN READ ORDER.
    ///
    /// Empty means UNDECLARED, which is a real and different statement from zero parameters:
    /// declaring inputs gives the capability pins in the designer and tells the compiler how to
    /// weave a node's arguments, while omitting them makes the compiler fall back to sorting pin
    /// names alphabetically — a convention the callee never agreed to, which silently reorders
    /// arguments the day a parameter is renamed. A genuinely variadic primitive declares nothing.
    #[serde(default)]
    pub inputs: Vec<DeclaredParameter>,
}

/// One parameter of a capability's calling contract. Its POSITION is the contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct DeclaredParameter {
    pub key: String,
    /// A WafflerDataType kind. Absent means `Dynamic`, which is accepted and defers every type
    /// error to runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

/// A frontend plugin this package ships, riding as an artifact inside its own bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct DeclaredUiPlugin {
    pub id: String,
    /// A frontend region name, or empty for a plugin that declares no slot.
    ///
    /// EMPTY IS THE HONEST VALUE FOR A PLUGIN THAT MOUNTS NOWHERE. The host reads `slot` only as the
    /// fallback for a contribution naming none, so a plugin contributing a route and a nav item
    /// never has it read at all — an invented name is INERT rather than rejected, which is exactly
    /// how one gets shipped and believed. This tool does NOT validate the name: a packer has no
    /// business knowing the frontend's regions, and coupling it to that list would make every new
    /// region a change here.
    #[serde(default)]
    pub slot: String,
    /// The declared artifact `name` (kind `UiBundle`) that IS the plugin.
    pub artifact: String,
}

/// A requested fast lane. `{target, secure}` — the reviewable ask.
#[derive(Debug, Clone, Serialize, Deserialize)]
// THE WIRE NAMES ARE camelCase BECAUSE A HUMAN WRITES THIS FILE. The Rust field names stay
// snake_case; a developer editing JSON should not have to know which language read it.
#[serde(rename_all = "camelCase")]
pub struct FastLaneRequest {
    pub target: String,
    #[serde(default)]
    pub secure: bool,
}

/// A declared artifact that has been found on disk after the build.
///
/// IT HOLDS A PATH, NOT BYTES, AND CARRIES NO HASH. A package artifact may be a hundred megabytes;
/// loading every one into memory to hand across a subsystem boundary buys nothing, because the only
/// component that needs the bytes is the one streaming them into the archive — and that is where
/// the hash is measured, over the bytes actually written.
#[derive(Debug, Clone)]
pub struct LocatedArtifact {
    /// The name this artifact takes inside the bundle, at `artifact/<name>`.
    pub name: String,
    pub kind: String,
    pub entry_point: Option<String>,
    pub absolute_path: std::path::PathBuf,
    /// Measured at location time so an oversized bundle can be refused before an upload rather than
    /// during one.
    pub size_bytes: u64,
}

/// A file from the project's `namespace/` tree, as a relative and absolute path pair.
#[derive(Debug, Clone)]
pub struct NamespaceFile {
    /// Forward-slashed, because it becomes a zip entry path and a backslash there is a filename
    /// component on every reader that matters.
    pub relative: String,
    pub absolute: std::path::PathBuf,
}

/// Everything needed to write a bundle, and nothing that requires having written one.
///
/// IT IS A PLAN, NOT A BUNDLE. No bytes, no hashes, no signature — those are facts about an archive
/// that does not exist yet, and a plan carrying them would be asserting them before they were true.
#[derive(Debug, Clone)]
pub struct BundlePlan {
    /// Repeated at the top level because the publish URL is built from it, and reaching into the
    /// manifest body for it would make the body's shape the publish route's problem.
    pub fqid: String,
    pub version: String,
    /// `uuidv5(NAMESPACE_OID, "waffler.package:<fqid>")`.
    pub package_uuid: String,
    /// The compiled `/.manifest` declarations, complete except for the artifacts array the writer
    /// fills in. Carried as opaque JSON on purpose: re-modelling core's `InstalledPackage` here
    /// would create a second definition of a format this tool does not own.
    pub manifest_body: serde_json::Value,
    pub artifacts: Vec<LocatedArtifact>,
    pub namespace_files: Vec<NamespaceFile>,
    /// Carried for messages only, so a failure can name the directory a developer is actually in.
    pub project_directory: std::path::PathBuf,
}

/// What a build did, in the terms of the tool that did it.
#[derive(Debug, Clone)]
pub struct BuildReport {
    /// Whether a toolchain was actually invoked, or existing artifacts were reused.
    ///
    /// REPORTED, ALWAYS. "It packed the wrong binary" and "it packed a binary it did not build" are
    /// the same incident seen a day apart, and only one of them is discoverable after the fact.
    pub ran: bool,
    pub crate_manifest: Option<String>,
    pub profile: String,
    pub succeeded: bool,
    /// The toolchain's own stdout and stderr, unchanged.
    pub output: String,
}

/// One thing wrong with an authored manifest: the field, and what is wrong with it.
///
/// A STRUCT RATHER THAN A STRING because the field is what a developer needs to find, and a message
/// that embeds it is a message something downstream has to parse to group by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub field: String,
    pub problem: String,
}

impl Violation {
    pub fn new(field: impl Into<String>, problem: impl Into<String>) -> Self {
        Self { field: field.into(), problem: problem.into() }
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.problem)
    }
}

/// A file a scaffold renders, before anything is written.
#[derive(Debug, Clone)]
pub struct RenderedFile {
    pub relative_path: String,
    pub contents: String,
}
