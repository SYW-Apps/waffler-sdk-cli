//! This crate's half of the message-whitespace guard.
//!
//! The predicate, the extractor and the walker live once in `tools/message-hygiene`. What stays here
//! is the part that is genuinely this crate's: floors measured against THIS tree, and go-red proofs
//! written from messages THIS crate actually produces.
//!
//! ## What this file replaced
//!
//! A 293-line copy, one of FOUR — `waffler_core/tests/`, `shared/src/message_hygiene.test.rs`,
//! `registry/tests/` and this one. Of their five functions, four were byte-identical in all four
//! copies and `gap_in_prose` had already drifted into two textual variants ({core, shared} against
//! {registry, sdk/cli}), semantically equivalent and textually different in four places. The
//! newline-padding exemption — which exists BECAUSE of this crate, where four of its multi-line
//! messages were the guard's first real-world findings and all four were false — had to be written
//! four times. That is the argument the consolidation rests on.
//!
//! The examples below are this crate's own, kept verbatim from the copy, because a failure is
//! legible in the crate it belongs to and the fixtures are strings the CLI really writes.

use std::path::PathBuf;

/// FROM THE COPY THIS REPLACES, not re-derived: `files > 20` and `examined > 1_500` are what this
/// crate's own guard asserted, so the conversion cannot quietly weaken the floor it inherited.
///
/// `examined` is the floor that catches a broken walker. `files` proves the walk found files; this
/// proves it read inside them, and a change that stopped the extractor working would leave the file
/// count untouched.
const MIN_FILES: usize = 21;
const MIN_EXAMINED: usize = 1_501;

#[test]
fn no_message_literal_carries_a_run_of_whitespace_between_words() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    message_hygiene::scan(&root).assert_clean(MIN_FILES, MIN_EXAMINED);
}

#[test]
fn the_predicate_still_detects_the_shape_it_exists_for() {
    // BUILT AT RUNTIME, NEVER AS A SOURCE LITERAL — the scan above reads this file too, so a
    // known-bad example written inline would be found by the guard meant to demonstrate it.
    let gap = " ".repeat(22);
    for message in [
        format!("the project declares no artifact{gap}named 'http.dll'"),
        format!("a grant is what an approval produces{gap}on a node"),
        format!("no publisher signing key.{gap}Run `waffler keygen` first"),
        format!("declare `pull` (or{gap}`auto` for a secret that never rotates)"),
        format!("resolve: `inputs` and{gap}`globals` where the unit has them"),
    ] {
        assert!(
            message_hygiene::gap_in_prose(&message).is_some(),
            "the shape this guard exists for must be detected: {message:?}"
        );
    }
}

#[test]
fn deliberate_alignment_is_not_flagged() {
    // A GUARD WHOSE FAILURES ARE ALL FALSE IS ONE SOMEBODY DELETES. Every case here is a shape the
    // CLI formats deliberately, and each one is a distinct reason the predicate declines: leading
    // and trailing padding, a brace after the gap, padding after a placeholder, an arrow, an equals.
    for aligned in [
        format!("  publishing:{} {{}}", " ".repeat(4)),
        format!("{}--registry <url>", " ".repeat(2)),
        format!("{}indented list item", " ".repeat(4)),
        format!("trailing padding{}", " ".repeat(4)),
        format!("key:{}{{value}}", " ".repeat(4)),
        format!("{{count}}{}items", " ".repeat(4)),
        format!("-->{}the next column", " ".repeat(4)),
        format!("total ={}42", " ".repeat(4)),
    ] {
        assert_eq!(
            message_hygiene::gap_in_prose(&aligned),
            None,
            "deliberate alignment must not be flagged: {aligned:?}"
        );
    }
}

#[test]
fn multi_line_advice_keeps_its_indentation() {
    // THE EXEMPTION THIS CRATE IS THE REASON FOR. Four of the CLI's messages were the guard's first
    // real findings and all four were legitimate: `\n` is two source characters, so the `n` reads as
    // a word and the next line's deliberate indent looked like prose with a gap in it.
    let indented = format!("the package does not exist.\\n{}resolved to nothing", " ".repeat(2));
    assert_eq!(message_hygiene::gap_in_prose(&indented), None);
}
