#![allow(non_snake_case)]
// Emphatic capitals in a test name are this codebase convention: a name that says WHAT IS
// BEING PROVEN reads better than one that obeys a lint, and a warning nobody clears becomes a
// warning nobody reads - which is how a real one gets missed.

//! Tests for the percent-encoding pair the authorize URL is built from, and for the callback
//! deadline.
//!
//! WHY ONLY THESE. The flow itself needs an issuer, a browser and a loopback round trip; exercising it
//! honestly means a live identity provider, which is what the end-to-end run against the development
//! provider does. What CAN be tested here in isolation is the encoding — and it is worth testing,
//! because a query value that is under-encoded in an authorize URL is a parameter-injection bug rather
//! than a cosmetic one.
//!
//! THE DEADLINE JOINED THAT LIST because it needs neither an issuer nor a browser: a listener nobody
//! connects to is the whole fixture. It was added after `clippy::never_loop` surfaced a blocking
//! accept that could not enforce its own timeout, so the CLI hung with no output when a developer
//! abandoned the browser tab. That is the one case the error message named.

use super::*;

#[test]
fn only_unreserved_characters_survive_unencoded() {
    assert_eq!(urlencode("abcXYZ019-_.~"), "abcXYZ019-_.~");
    // A value carrying `&` or `=` would otherwise ADD a parameter to the authorize URL — which is the
    // whole reason this is not a passthrough.
    assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
    assert_eq!(urlencode("https://r.example/cb"), "https%3A%2F%2Fr.example%2Fcb");
    assert_eq!(urlencode(" "), "%20");
}

#[test]
fn decoding_reverses_encoding_and_handles_the_form_plus_convention() {
    for raw in ["abc", "a&b=c", "https://127.0.0.1:1234/callback", "a b", "état"] {
        assert_eq!(urldecode(&urlencode(raw)), raw, "round trip failed for {raw}");
    }
    // A form-encoded query spells a space as `+`, and an authorization server may return one in a state
    // or error value. Decoding it as a literal plus would make a state comparison fail on a value that
    // matched.
    assert_eq!(urldecode("a+b"), "a b");
}

#[test]
fn a_truncated_percent_escape_is_left_alone_rather_than_dropping_bytes() {
    // A malformed redirect is attacker-influenced input. Silently dropping the tail would make two
    // different callbacks decode to the same state string, and a state comparison is the check that
    // stops another local process completing the flow with a code for a different account.
    assert_eq!(urldecode("abc%"), "abc%");
    assert_eq!(urldecode("abc%z9"), "abc%z9");
}

#[test]
fn the_callback_deadline_FIRES_when_the_browser_never_comes_back() {
    // The fixture is a listener and nothing else: no issuer, no browser, no connection. Before the
    // fix this parked forever on `listener.incoming()` and the test would have hung rather than
    // failed, which is why the elapsed time is asserted as well as the error.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let budget = std::time::Duration::from_millis(300);
    let started = std::time::Instant::now();

    // ON A THREAD, READ BACK WITH A TIMEOUT, so that a deadline which stops firing FAILS this test
    // instead of hanging it. Called directly, the assertions below are only reached once the call
    // returns — so the exact regression being guarded (an accept that never gives up) would stall
    // the test runner until CI killed the job, and a hang reports nothing at all. A hang is the
    // worst kind of absent verdict: it does not even say which test was the problem.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(wait_for_code_until(
            listener,
            "the-state-this-process-issued",
            started + budget,
        ));
    });
    let outcome = rx
        .recv_timeout(budget * 20)
        .expect("it never gave up: the callback deadline is not being honoured");
    let elapsed = started.elapsed();

    let error = outcome.expect_err("a callback that never arrives must not succeed");
    assert!(
        error.to_string().contains("did not come back"),
        "the refusal must name the timeout, got: {error}"
    );
    // THE LOWER BOUND IS THE ASSERTION HERE. Under it, the call would be returning without waiting
    // at all — a different bug wearing the same error message. The upper bound is enforced by the
    // `recv_timeout` above rather than by a comparison after the fact, which is what turns a hang
    // into a failure. Generous either way, because a loaded CI box is not a clock: the claim is "it
    // gave up near its deadline", not a precise interval.
    assert!(elapsed >= budget, "it gave up after {elapsed:?}, before its own {budget:?} deadline");
}
