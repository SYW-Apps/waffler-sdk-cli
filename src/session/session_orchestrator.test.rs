//! Tests for registry resolution and URL normalisation.
//!
//! THE RESOLUTION ORDER IS TESTED THROUGH THE ENVIRONMENT, which means these tests must not run
//! concurrently with each other while mutating it. They are serialised through one mutex rather than
//! marked `#[ignore]`, because a test that does not run is a test that is not protecting anything.

use super::*;
use crate::session::types::{PersistedSession, RegistrySource};

/// Cargo runs tests in threads within one process, and `std::env` is process-global — so two tests
/// setting WAFFLER_REGISTRY at once would each see the other's value and both would be flaky in a way
/// that looks like a resolution bug.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run a closure with the environment variables this module reads set to known values.
fn with_env<T>(registry: Option<&str>, token: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match registry {
        Some(v) => std::env::set_var(ENV_REGISTRY, v),
        None => std::env::remove_var(ENV_REGISTRY),
    }
    match token {
        Some(v) => std::env::set_var(ENV_TOKEN, v),
        None => std::env::remove_var(ENV_TOKEN),
    }
    let out = f();
    std::env::remove_var(ENV_REGISTRY);
    std::env::remove_var(ENV_TOKEN);
    out
}

fn session_with_default(default: Option<&str>) -> PersistedSession {
    PersistedSession { default_registry: default.map(str::to_string), credentials: Default::default() }
}

#[test]
fn resolution_prefers_a_flag_then_the_environment_then_the_default_then_the_builtin() {
    let session = session_with_default(Some("https://persisted.example"));

    // THE MOST EXPLICIT WINS, which is what makes a one-off publish to a different registry possible
    // without disturbing a persisted default.
    with_env(Some("https://from-env.example"), None, || {
        let r = resolve_registry(&session, Some("https://from-flag.example")).unwrap();
        assert_eq!(r.base_url, "https://from-flag.example");
        assert_eq!(r.source, RegistrySource::Flag);
    });

    with_env(Some("https://from-env.example"), None, || {
        let r = resolve_registry(&session, None).unwrap();
        assert_eq!(r.base_url, "https://from-env.example");
        assert_eq!(r.source, RegistrySource::Environment);
    });

    with_env(None, None, || {
        let r = resolve_registry(&session, None).unwrap();
        assert_eq!(r.base_url, "https://persisted.example");
        assert_eq!(r.source, RegistrySource::Default);
    });

    // LAST, NOT FIRST. A compiled-in registry as the primary answer is exactly the assumption this
    // subsystem exists to remove.
    with_env(None, None, || {
        let r = resolve_registry(&session_with_default(None), None).unwrap();
        assert_eq!(r.base_url, BUILTIN_REGISTRY);
        assert_eq!(r.source, RegistrySource::Builtin);
    });
}

#[test]
fn a_blank_flag_or_environment_value_does_not_win() {
    let session = session_with_default(Some("https://persisted.example"));
    // An exported-but-empty variable is a shell accident, not a choice. Treating it as a choice would
    // resolve to an unusable URL and refuse with a message about the URL rather than about the variable.
    with_env(Some("   "), None, || {
        let r = resolve_registry(&session, Some("")).unwrap();
        assert_eq!(r.base_url, "https://persisted.example");
        assert_eq!(r.source, RegistrySource::Default);
    });
}

#[test]
fn every_source_reports_a_reason_so_a_developer_can_see_where_it_came_from() {
    // "publishing to X" answers a different question from "publishing to X, because WAFFLER_REGISTRY is
    // set in this shell", and only the second one lets a developer notice before the upload.
    for source in [RegistrySource::Flag, RegistrySource::Environment, RegistrySource::Default, RegistrySource::Builtin] {
        assert!(!source.because().is_empty());
    }
}

#[test]
fn normalisation_reduces_every_spelling_of_one_registry_to_one_key() {
    // THE CREDENTIAL STORE KEYS ON THIS STRING. Two spellings holding two credentials means the request
    // that went to the wrong one carried a valid bearer for somewhere else.
    for (raw, expected) in [
        ("https://Registry.Waffler.DEV", "https://registry.waffler.dev"),
        ("https://registry.waffler.dev/", "https://registry.waffler.dev"),
        ("https://registry.waffler.dev///", "https://registry.waffler.dev"),
        // A default port written out is the same registry as one left off.
        ("https://registry.waffler.dev:443", "https://registry.waffler.dev"),
        ("http://localhost:80", "http://localhost"),
        // A NON-default port is part of the identity and must survive.
        ("http://localhost:42070", "http://localhost:42070"),
        ("HTTPS://registry.waffler.dev", "https://registry.waffler.dev"),
    ] {
        assert_eq!(normalise_registry_url(raw).unwrap(), expected, "for {raw}");
    }
}

#[test]
fn a_path_keeps_its_case_because_a_path_is_case_sensitive() {
    // Lowercasing the whole URL would silently rewrite a registry mounted under a mixed-case prefix into
    // one that answers 404 — and the failure would look like the registry being down.
    assert_eq!(
        normalise_registry_url("https://Host.Example/MyRegistry/").unwrap(),
        "https://host.example/MyRegistry"
    );
}

#[test]
fn a_url_with_no_scheme_or_no_host_is_refused_naming_which_rule_it_broke() {
    let e = normalise_registry_url("registry.waffler.dev").unwrap_err().to_string();
    assert!(e.contains("absolute"), "got {e}");

    let e = normalise_registry_url("https:///v1").unwrap_err().to_string();
    assert!(e.contains("host"), "got {e}");
}

#[test]
fn a_credential_is_fresh_only_with_room_to_spare() {
    let base = crate::session::types::RegistryCredential {
        registry: "https://r.example".into(),
        access_token: "t".into(),
        refresh_token: None,
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(60),
        subject: "s".into(),
        username: None,
        issuer: "https://i.example".into(),
        client_id: "waffler-cli".into(),
    };
    // A TOKEN THAT EXPIRES BETWEEN THE CHECK AND A LARGE UPLOAD WASTES THE UPLOAD. Sixty seconds of life
    // is "valid" and is NOT enough margin, which is the whole reason the margin exists.
    assert!(base.is_fresh(chrono::Duration::zero()));
    assert!(!base.is_fresh(chrono::Duration::seconds(120)));
}
