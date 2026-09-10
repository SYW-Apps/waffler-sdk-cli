#![allow(non_snake_case)]
// Emphatic capitals in a test name are this codebase convention: a name that says WHAT IS
// BEING PROVEN reads better than one that obeys a lint, and a warning nobody clears becomes a
// warning nobody reads - which is how a real one gets missed.

//! Tests for the publisher key.

use super::*;

#[test]
fn a_generated_key_round_trips_and_yields_the_public_half_it_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("publisher.key");

    let reported = generate_signing_key(&path).unwrap();
    let seed = load_signing_key(&path).unwrap();

    // THE PUBLIC HALF THE COMMAND PRINTED MUST BE THE ONE THE STORED SEED PRODUCES. A developer
    // records that string as who they publish as; if it were derived from anything else, they would be
    // recording an identity nothing ever signs with — and would find out at the first pinned update, on
    // somebody else's node.
    let derived = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    assert_eq!(reported, derived);
}

#[test]
fn a_key_is_NEVER_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("publisher.key");
    generate_signing_key(&path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();

    let e = generate_signing_key(&path).unwrap_err().to_string();
    // Replacing a publisher key destroys the only copy of something that cannot be reissued: every
    // node that installed the package pins the old publisher, rotation is refused, and the package
    // becomes permanently un-updatable on all of them.
    assert!(e.contains("already exists"), "got {e}");
    assert!(e.to_lowercase().contains("pin"), "the refusal must say WHY, not just that it refused: {e}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "the existing key must be untouched");
}

#[test]
fn a_missing_key_names_the_PATH_rather_than_complaining_about_its_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nowhere.key");
    let e = load_signing_key(&path).unwrap_err().to_string();
    // A key that is not where the developer said it is is the ordinary mistake, and a message about
    // decoding would send them to look at contents that are not there.
    assert!(e.contains("nowhere.key"), "got {e}");
    assert!(e.contains("no publisher signing key"), "got {e}");
}

#[test]
fn a_seed_of_the_wrong_length_is_refused_naming_BOTH_lengths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.key");
    std::fs::write(&path, hex::encode([1u8; 16])).unwrap();
    let e = load_signing_key(&path).unwrap_err().to_string();
    // A library that padded or truncated a seed would produce a key that signs consistently and
    // matches nothing — surfacing as a signature that verifies nowhere, naming no file.
    assert!(e.contains("16-byte"), "the length found: {e}");
    assert!(e.contains("32"), "and the length required: {e}");
}

#[test]
fn a_file_that_is_not_hex_is_refused_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("junk.key");
    std::fs::write(&path, "this is not a key").unwrap();
    let e = load_signing_key(&path).unwrap_err().to_string();
    assert!(e.contains("junk.key"), "got {e}");
}

#[test]
fn surrounding_whitespace_does_not_change_the_key() {
    // An editor that adds a trailing newline must not change who a developer publishes as. Without the
    // trim this decodes as junk, and the failure would read as a corrupt key rather than as a newline.
    let dir = tempfile::tempdir().unwrap();
    let clean = dir.path().join("clean.key");
    let padded = dir.path().join("padded.key");
    let seed = [7u8; SEED_LEN];
    std::fs::write(&clean, hex::encode(seed)).unwrap();
    std::fs::write(&padded, format!("  {}\n", hex::encode(seed))).unwrap();
    assert_eq!(load_signing_key(&clean).unwrap(), load_signing_key(&padded).unwrap());
}

#[cfg(unix)]
#[test]
fn a_world_readable_key_is_REFUSED_rather_than_warned_about() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loose.key");
    std::fs::write(&path, hex::encode([3u8; SEED_LEN])).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let e = load_signing_key(&path).unwrap_err().to_string();
    // A signing key anyone on the machine can read belongs to anyone on the machine, and using it
    // anyway would treat the developer's mistake as consent. A warning on a path that then succeeds is
    // a warning nobody reads twice.
    assert!(e.contains("readable by other users"), "got {e}");
    assert!(e.contains("chmod 600"), "and it must say how to fix it: {e}");

    // THE POSITIVE HALF. Without it, a check that refused every key would pass the assertion above.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(load_signing_key(&path).is_ok());
}

#[cfg(unix)]
#[test]
fn a_generated_key_is_owner_only_from_the_moment_it_exists() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("publisher.key");
    generate_signing_key(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    // Created with the permission rather than tightened afterwards: a file created with defaults and
    // chmod'd later is world-readable for the length of that window, and a private key is exactly the
    // file for which the window matters. That this LOADS is the proof it is not group- or
    // other-readable, since loading refuses one that is.
    assert_eq!(mode, 0o600, "got {mode:o}");
    assert!(load_signing_key(&path).is_ok());
}

#[test]
fn the_commitment_states_all_three_consequences() {
    // A CONSTANT RATHER THAN A println AT THE CALL SITE, so the statement cannot drift away from the
    // operation that makes it true. What it has to say is the whole reason this is a one-way door.
    let text = COMMITMENT.to_lowercase();
    assert!(text.contains("pins this publisher"), "the pin: {COMMITMENT}");
    assert!(text.contains("refused"), "that a different key is refused: {COMMITMENT}");
    assert!(text.contains("never be updated"), "and what losing it costs: {COMMITMENT}");
}
