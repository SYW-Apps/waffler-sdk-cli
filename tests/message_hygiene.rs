//! A guard against runs of whitespace inside user-facing message literals.
//!
//! The CLI's copy of the check that lives in `waffler_core/tests/message_hygiene.rs`,
//! `shared/src/message_hygiene.test.rs` and `registry/tests/message_hygiene.rs`. Duplicated rather
//! than shared for the reason stated there:
//! a test that walks ITS OWN crate's source cannot be handed a directory from elsewhere without
//! becoming a tool, and a nine-line predicate is less to keep honest than a tool would be.
//!
//! ## The defect
//!
//! A message written across source lines whose `\` continuation did not survive an edit leaves the
//! SOURCE INDENTATION inside the rendered string:
//!
//! ```text
//! requests `bus:register_middleware`, which waffler_core grants through                      the host's own group
//! ```
//!
//! It is invisible in the source, where it looks like a deliberate line break, and invisible to
//! tests unless something asserts on the rendered form. Twenty-eight were found in core and shared
//! when this check was first written, every one of them prose a person reads at the moment something
//! has gone wrong.
//!
//! ## Why the CLI needs its own
//!
//! This crate is nothing BUT prose a person reads: every `validate` violation, every pack refusal and
//! every advisory is text an author acts on, and the advisories are the longest messages in any of
//! these repositories — several are three continued lines explaining which node grants what. A run of
//! spaces in one of those is a formatting bug in the middle of advice. This crate had none when the
//! check was first run here; the point is not the sweep, it is that nothing stopped the next one.
//!
//! ## Why it scans SOURCE and not rendered output
//!
//! The strings most likely to carry it are the rarest: a bundle whose declared artifact is absent, a
//! reserved group id, a key file the loader refuses. A rendered-output test only covers what a test
//! can reach, and these are the paths nothing reaches.
//!
//! ## Why the predicate is narrow, and where it was BLIND
//!
//! It matches only a gap that fell BETWEEN WORDS: a word character or sentence punctuation, then two
//! or more spaces, then a word character or a quote. Deliberate alignment (`"  key:  {}"`,
//! `"{:<20} {}"`) does not match, and there is a case below proving it — **a guard whose failures are
//! all false is one someone deletes**, which makes it the worst way for a check to fail.
//!
//! The narrowness hid something. Neither character class contained a BACKTICK, so in a codebase that
//! quotes every identifier in backticks, the commonest shape of all was silently exempt. waffler_core
//! found it when a known run went unreported, assumed the walker's reach was wrong, and discovered
//! the walker had been reading the whole tree correctly the entire time: the instrument was the
//! fault. Widening both clauses immediately surfaced a second defect nobody knew about. Both classes
//! here include the backtick from the start, and three of the cases below exist for it.

use std::path::{Path, PathBuf};

/// A word/punctuation character, two-or-more spaces, then a word/quote character.
fn gap_in_prose(literal: &str) -> Option<usize> {
    let chars: Vec<char> = literal.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' && i + 1 < chars.len() && chars[i + 1] == ' ' {
            let start = i;
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            if start == 0 || i >= chars.len() {
                continue; // leading or trailing padding is alignment, not prose
            }
            // PADDING AFTER A SOURCE-FORM NEWLINE IS INDENTATION, exactly like padding at the start
            // of the literal — this scans SOURCE, so a `\n` is the two characters backslash and `n`,
            // and the `n` reads as a word character to everything below. `"...exist.\n  resolved to"`
            // is a deliberately indented second line, and the CLI writes its multi-line advice that
            // way throughout: four of its messages were reported the first time this ran, all four
            // legitimate. A guard whose failures are all false is one someone deletes.
            if start >= 2 && chars[start - 1] == 'n' && chars[start - 2] == '\\' {
                continue;
            }
            let before = chars[start - 1];
            let after = chars[i];
            // THE BACKTICKS ARE NOT DECORATION IN EITHER CLASS. Without them a gap beside a quoted
            // identifier — which is nearly every message this crate writes — is exempt.
            let before_ok = before.is_alphanumeric() || ".,;:)]'`".contains(before);
            let after_ok =
                after.is_alphanumeric() || after == '\'' || after == '"' || after == '(' || after == '`';
            if before_ok && after_ok {
                return Some(start);
            }
            continue;
        }
        i += 1;
    }
    None
}

/// Every string literal on a line, ignoring escaped quotes.
fn literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            let mut body = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    body.push(chars[i]);
                    i += 1;
                }
                body.push(chars[i]);
                i += 1;
            }
            out.push(body);
        }
        i += 1;
    }
    out
}

/// Whether a source line is a comment, and therefore not a message.
///
/// EXTRACTED FROM THE WALKER rather than left as an inline `continue`, because inline it would be
/// unkillable by construction: no test can hand a value to a decision that only exists during an
/// iteration, so neutralising it leaves the suite green while it silently decides what half the tree
/// meant.
fn is_comment_line(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// Whether a path is Rust source. Named for the same reason as [`is_comment_line`].
fn is_rust_source(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "rs")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if is_rust_source(&path) {
            out.push(path);
        }
    }
}

#[test]
fn no_message_literal_carries_a_run_of_whitespace_between_words() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);

    // RECURSION, ASSERTED EXACTLY. A file COUNT catches a broken walker only by luck — it depends on
    // how many files happen to sit at the top level, and this crate keeps 34 of its 37 files in
    // subsystem directories, so a walker that read only `src/*.rs` would scan three files and pass.
    assert!(
        files.iter().any(|p| p.parent().is_some_and(|d| d.file_name().is_some_and(|n| n != "src"))),
        "every scanned file is directly under src/, so the walker is not recursing and most of this \
         crate is unread"
    );
    assert!(files.len() > 20, "the scan found only {} files; it is not reading the tree", files.len());

    let mut findings = Vec::new();
    // COUNT WHAT WAS ACTUALLY EXAMINED. `files.len()` proves the walker found FILES; it says nothing
    // about whether anything was extracted from them, and a scan that reads every file and pulls no
    // literals out of any of them is GREEN.
    let mut examined = 0usize;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        for (n, line) in text.lines().enumerate() {
            if is_comment_line(line) {
                continue;
            }
            for literal in literals(line) {
                examined += 1;
                if gap_in_prose(&literal).is_some() {
                    findings.push(format!("{}:{}\n      {}", path.display(), n + 1, literal.trim()));
                }
            }
        }
    }

    assert!(
        examined > 1_500,
        "the scan examined only {examined} string literal(s) across {} files. This crate has more than \
         twice that, so the extractor or the line filter is dropping them and this check is passing \
         without reading anything.",
        files.len()
    );

    assert!(
        findings.is_empty(),
        "{} message literal(s) carry a run of whitespace between words. This is almost always a \
         string continuation that did not survive an edit, and it renders as a formatting bug in the \
         middle of advice an author reads while their package will not pack. Put the literal on one \
         line, or use a `\\` continuation and check the rendered form.\n\n{}",
        findings.len(),
        findings.join("\n\n")
    );
}

/// THE PAIR. Without this the scan above would pass just as well against a predicate that matches
/// nothing — which is what a guard degrades into once someone narrows it to silence a false positive.
#[test]
fn the_predicate_still_detects_the_shape_it_exists_for() {
    // BUILT AT RUNTIME, never written literally. A fixture spelled out here would be a real run of
    // spaces in this file's own source, and this file is scanned like any other — so constructing the
    // gap is what lets the guard's own messages be checked by it.
    let gap = " ".repeat(6);
    assert!(gap_in_prose(&format!("the project declares no artifact{gap}named 'http.dll'")).is_some());
    assert!(gap_in_prose(&format!("a grant is what an approval produces{gap}on a node")).is_some());
    // ...and after a sentence, which is where a continuation usually sits.
    assert!(gap_in_prose(&format!("no publisher signing key.{gap}Run `waffler keygen` first")).is_some());
    // A BACKTICK ON EITHER SIDE OF THE RUN — the shape that was exempt until waffler_core found it,
    // and the most likely one here, because every message in this crate quotes names in backticks.
    assert!(gap_in_prose(&format!("declare `pull` (or{gap}`auto` for a secret that never rotates)")).is_some());
    assert!(gap_in_prose(&format!("resolve: `inputs` and{gap}`globals` where the unit has them")).is_some());
    // ...and a CLOSING backtick on the near side, the same shape mirrored onto the `before` clause,
    // which is the clause a mutation run found had no cases of its own.
    assert!(gap_in_prose(&format!("only `inputs`{gap}resolves in this unit")).is_some());

    // A GAP AFTER A SOURCE-FORM NEWLINE IS NOT ONE, and the pair below is what keeps that exemption
    // from swallowing the defect it sits next to. The first is deliberate indentation of a second
    // line; the second is a real run in the middle of that same second line, which must still fire.
    assert!(gap_in_prose(r"does not exist.\n  resolved to: /tmp/x").is_none());
    assert!(gap_in_prose(&format!(r"does not exist.\n  resolved{gap}to: /tmp/x")).is_some());
}

/// DELIBERATE ALIGNMENT IS NOT THE DEFECT, and this is the half that keeps the guard alive. A check
/// that cries wolf gets deleted rather than narrowed.
///
/// WHAT THIS LIST DOES NOT CLAIM. Every case here is one the predicate genuinely spares, and the
/// distinction is load-bearing: a column aligned as WORD, spaces, WORD — `"--registry    the registry
/// to publish to"` — is indistinguishable from prose with a gap in it, and IS flagged. That is the
/// price of catching the defect at all, and the first version of this list asserted otherwise, which
/// would have been a false promise about the predicate rather than a test of it. Align with leading
/// padding, a format spec (`{:<20}`), or a placeholder, all of which are below.
#[test]
fn deliberate_alignment_is_not_flagged() {
    for aligned in [
        format!("  publishing:{} {{}}", " ".repeat(4)),            // a CLI column
        "{:<20} {}".to_string(),                                   // a format spec
        // Help indented by LEADING padding only. The version of this case with the description
        // aligned into a second column — `"  --registry   the registry"` — was written twice here and
        // flagged both times, correctly: the run sits between two words and nothing can tell it from
        // prose. That is the limitation the doc comment above states, and this is the shape that
        // respects it.
        format!("{}--registry <url>", " ".repeat(2)),
        format!("{}indented list item", " ".repeat(4)),            // leading padding
        format!("trailing padding{}", " ".repeat(4)),              // trailing padding
        format!("key:{}{{value}}", " ".repeat(4)),                 // a colon, padding, then a brace
        // THE `before` CLAUSE'S OWN CASES. Without them, relaxing that clause to `true` leaves the
        // suite green: every other negative here happens to have a `{` or an `=` after the run, so a
        // different clause does all the work. A clause no test reaches can be deleted without
        // anything going red.
        format!("{{count}}{}items", " ".repeat(4)),                // padding after a format placeholder
        format!("-->{}the next column", " ".repeat(4)),            // padding after an arrow
        format!("total ={}42", " ".repeat(4)),                     // padding after an equals
    ] {
        assert!(gap_in_prose(&aligned).is_none(), "alignment must not be flagged: {aligned:?}");
    }
}

/// THE EXTRACTOR, tested directly. The scan's own floor catches a total collapse; this catches the
/// subtler shape where it still returns something but the wrong thing.
#[test]
fn the_extractor_pulls_literals_out_of_a_line() {
    assert_eq!(literals(r#"let x = "hello";"#), vec!["hello".to_string()]);
    assert_eq!(
        literals(r#"format!("a {}", "b")"#),
        vec!["a {}".to_string(), "b".to_string()],
        "every literal on the line, not just the first"
    );
    // An escaped quote must not end the literal early — otherwise the rest of a message is silently
    // outside what gets checked.
    assert_eq!(literals(r#"let x = "say \"hi\" now";"#), vec![r#"say \"hi\" now"#.to_string()]);
    assert!(literals("let x = 5;").is_empty());
}

/// THE TWO FILTERS, handed values directly, because as inline `continue`s they were unkillable.
#[test]
fn the_walker_filters_are_reachable_and_do_what_they_say() {
    assert!(is_comment_line("// an ordinary comment"));
    assert!(is_comment_line("    //! an indented doc comment"));
    assert!(!is_comment_line(r#"let x = "not a comment";"#));
    // BOTH: a comment that also contains a quoted string with a gap in it. Either filter alone
    // rejects the other's fixture, which is exactly why they were unkillable while fused.
    let gap = " ".repeat(6);
    assert!(is_comment_line(&format!(r#"    // see "a{gap}b" for the shape"#)));

    assert!(is_rust_source(Path::new("src/lib.rs")));
    // This crate has no non-Rust file under src/ today, but a fixture sitting beside code is an
    // ordinary shape and reachable-but-absent is not unreachable. Deleting the clause would make the
    // guard cry wolf the first time somebody adds one.
    assert!(!is_rust_source(Path::new("src/project/scaffold_vectors.json")));
    assert!(!is_rust_source(Path::new("src/README")));
}
