//! The Waffler package developer SDK, as a library.
//!
//! Spec tree: `.wai/specs/` — `sdk_cli::{project, session, publishing, distribution}`.
//!
//! ## WHY THERE IS A LIBRARY AT ALL
//!
//! The docker image build packs bundles by calling this crate, not by shelling out to an installed
//! `waffler` binary. That is what forces every portal function to be reachable without a process, and
//! the discipline it buys is concrete: a check written in a command handler is a check the library path
//! silently skips — and the library path is the one our own image build uses, so the skipped check would
//! be skipped exactly where it matters most.
//!
//! It also removes the tool's own worst failure mode. The previous CLI was the only way to pack a
//! package, it had not built in four months, and nothing in the tree noticed — because nothing in the
//! tree depended on it. A library the image build links is a build that fails when this code does.
//!
//! ## THE THREE ENTRY POINTS
//!
//! ```text
//!   project::project_portal     plan | validate | build | scaffold
//!   session::session_portal     resolve | bearer | profile | login | logout | whoami | use
//!   publishing::publish_portal  pack | publish | unpublish
//! ```
//!
//! Nothing below a portal is meant to be called from outside; the modules are public so the specs'
//! component names and the code's module names are the same words, which is what lets a reader move
//! between them without a map.

pub mod project;
pub mod publishing;
pub mod session;

/// A shared HTTP client.
///
/// ONE CLIENT PER PROCESS, because a client owns a connection pool and a TLS session cache — building
/// one per request throws both away and turns a publish into a fresh handshake per call.
///
/// NO GLOBAL TIMEOUT, and that is deliberate rather than an omission. A publish uploads a bundle that
/// may be a hundred megabytes over an arbitrary link, so any wall-clock ceiling that is safe for a
/// metadata read is one that kills a legitimate upload partway — and a partial upload is the failure a
/// developer can least diagnose. Connect timeouts bound the case that actually hangs, which is a host
/// that never answers.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("waffler-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        // A client that cannot be constructed means the TLS backend is unavailable, which is not a
        // condition any caller can act on differently from any other.
        .expect("the HTTP client could not be built, which means the TLS backend is unavailable")
}

#[cfg(test)]
#[path = "message_shape.test.rs"]
mod message_shape_tests;
