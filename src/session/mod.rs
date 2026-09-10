//! `sdk_cli::session` — which registry this tool is talking to, and who it is talking as.
//!
//! Spec: `sdk_cli::session`.
//!
//! THE REGISTRY IS AN ARGUMENT, NOT A CONSTANT, and this subsystem exists because the previous tool
//! treated it as one. Two hosts were compiled in, so the tool could not address a private or
//! self-hosted registry at all — the ordinary case for this product rather than an advanced one, and
//! the same limitation the marketplace package removed on the node side.
//!
//! CREDENTIALS ARE PER REGISTRY, NEVER GLOBAL. Signing in to one registry says nothing about
//! another: they are different services with different identity providers and different namespace
//! ownership. A single cached token would let a `use` change silently carry an identity across a
//! trust boundary, which is the confused-deputy shape one level out from the tool.
//!
//! AUTHENTICATION IS DISCOVERED, NOT COMPILED IN. A registry advertises its own issuer, and this
//! subsystem authenticates against whatever that instance names — which is what makes "sign in to my
//! own registry" work without this tool knowing anything about the operator's identity provider in
//! advance.
//!
//! IT NEVER DECIDES WHAT A TOKEN IS ALLOWED TO DO. The registry answers that, and a 401 or 403 is
//! reported as the registry worded it rather than reinterpreted.

pub mod oidc_adapter;
pub mod registry_metadata_adapter;
pub mod session_orchestrator;
pub mod session_portal;
pub mod session_store;
pub mod types;
