//! `sdk_cli::publishing` — turning a built project into an artifact a registry has accepted.
//!
//! Spec: `sdk_cli::publishing`.
//!
//! WHAT IT OWNS: the bundle format as WRITTEN — `/.manifest` + `/artifact/<name>` +
//! `/namespace/<uuid>.json` — and the HTTP conversation with a registry's publication API. It is the
//! producer counterpart to `registry::publication`, which reads and validates everything written here,
//! and to `waffler_core`'s installer, which verifies it again on a node months later.
//!
//! THE INVARIANT IT SERVES: the bytes a node verifies must be the bytes that were signed. Nothing here
//! may re-zip, re-compress or normalise a payload after signing it — a transformation that looks
//! harmless produces an artifact core refuses as IntegrityFailure, discovered on a stranger's machine
//! rather than here. The registry holds the same invariant from the other side and calls it opaque
//! custody.
//!
//! TWO PUBLISH SHAPES, and the tool must make the safe one the easy one. A bundle uploaded UNSIGNED is
//! signed by the registry alone; a bundle carrying a publisher signature would be countersigned. The
//! registry refuses an upload that already carries a REGISTRY signature, because it cannot safely decide
//! which trailing bytes were signature and which were content — so producing the right artifact for the
//! right path is this subsystem's job rather than the operator's.
//!
//! WHAT IT DOES NOT DO. It does not install, does not verify a node's trust anchors, and does not decide
//! whether a package is trustworthy. It produces an artifact and hands it to a registry; every
//! acceptance decision after that belongs to someone else.

pub mod bundle_writer;
pub mod project_client_adapter;
pub mod publication_adapter;
pub mod publish_orchestrator;
pub mod publish_portal;
pub mod publisher_key_adapter;
pub mod session_client_adapter;
pub mod signature_specialist;
pub mod types;
