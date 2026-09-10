//! Tests for the percent-encoding pair the authorize URL is built from.
//!
//! WHY ONLY THESE. The flow itself needs an issuer, a browser and a loopback round trip; exercising it
//! honestly means a live identity provider, which is what the end-to-end run against the development
//! provider does. What CAN be tested here in isolation is the encoding — and it is worth testing,
//! because a query value that is under-encoded in an authorize URL is a parameter-injection bug rather
//! than a cosmetic one.

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
