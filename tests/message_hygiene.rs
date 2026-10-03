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

/// RE-DERIVED IN THE NEW UNIT, and the first version of this was wrong in the most embarrassing
/// available way.
///
/// The copy this replaced counted LITERALS — `examined += 1` per literal, floor `> 1_500` against
/// 1,576 actual. The hosted crate counts CHARACTERS. I carried `1_500` across and wrote "FROM THE
/// COPY THIS REPLACES, not re-derived ... so the conversion cannot quietly weaken the floor it
/// inherited" directly above it. At 18.2 characters per literal in this tree, that floor was
/// satisfied by about 82 literals where the old one demanded 1,500: **roughly nineteen times weaker,
/// under a comment asserting exactly the property the change had broken.** waffler_core caught it.
/// The field is now `examined_chars` so the unit is unavoidable at the call site.
///
/// MEASURED: 37 files, 28,638 literal characters. The floors sit at ~80% of that — enough headroom
/// for ordinary deletion, far above zero.
///
/// THE OLD FLOOR'S TIGHTNESS IS DELIBERATELY NOT REPRODUCED. `1_500` of 1,576 literals left 5%
/// slack, so deleting a handful of messages would have broken it — and a floor that fails when
/// somebody tidies up is a floor that gets lowered rather than investigated.
const MIN_FILES: usize = 30;
const MIN_EXAMINED_CHARS: usize = 23_000;

#[test]
fn no_message_literal_carries_a_run_of_whitespace_between_words() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    message_hygiene::scan(&root).assert_clean(MIN_FILES, MIN_EXAMINED_CHARS);
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
