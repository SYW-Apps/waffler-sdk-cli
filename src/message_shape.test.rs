#![allow(non_snake_case)]
// Emphatic capitals in a test name are this codebase convention: a name that says WHAT IS
// BEING PROVEN reads better than one that obeys a lint, and a warning nobody clears becomes a
// warning nobody reads - which is how a real one gets missed.

//! One guard over every user-facing string this crate contains.
//!
//! ## THE DEFECT THIS EXISTS FOR HAS HAPPENED SEVEN TIMES
//!
//! A message written across source lines uses a trailing backslash so the newline and the following
//! indentation are stripped:
//!
//! ```text
//!     bail!("the archive declares {n} bytes, over the \
//!            limit for bundle metadata")
//! ```
//!
//! When an edit loses that backslash — and several tools eat it — the literal keeps the source's own
//! indentation, and the message ships as `over the                limit`. Five of these were found and
//! collapsed in the registry in one afternoon, and core shipped one inside a security refusal on the
//! very change whose purpose was making that refusal readable.
//!
//! IT IS INVISIBLE EVERYWHERE EXCEPT THE RENDERED OUTPUT. The source looks like a deliberate line
//! break. No compiler warns, no test that asserts on a substring notices, and a reviewer reading the
//! diff sees what the author meant rather than what it says. Core's generalisation, taken as a rule
//! here: **a message assembled across source lines is one whose rendered form nothing checks unless
//! something asserts on it.**
//!
//! ## WHY THE SOURCE RATHER THAN THE RENDERED MESSAGES
//!
//! Asserting on rendered output would need every refusal to be reachable from a test, and the ones
//! most likely to carry this defect are the rarest — a decompression bomb, an ambiguous archive end, a
//! world-readable key. A source scan covers the strings that no test will ever render, which is
//! exactly where the bug survives.

use std::path::Path;

/// A run of spaces long enough that nothing wrote it on purpose mid-sentence.
///
/// FOUR RATHER THAN TWO, because two is deliberate here: several messages indent a follow-up line as
/// `\n  Run: waffler login`, and a bulleted commitment uses `  * `. A threshold that flagged those
/// would be a guard whose failures are all false, which is a guard that gets deleted.
const RUN: usize = 4;

/// Does this line carry a long run of spaces that split a SENTENCE?
///
/// ## THE FIRST VERSION FLAGGED NINE LINES AND ALL NINE WERE FINE
///
/// It asked only whether a run sat between two non-space characters, which is true of every aligned
/// column this tool prints — `"  publishing:     {}"`, `"  sign in via:    {url}"` — and of every
/// indented list item, `"    {}"`. A guard whose failures are all false is a guard someone deletes,
/// and deleting it is the correct response to it, which is the worst way for a check to fail.
///
/// So the signature is narrowed to what the defect actually looks like: a gap between WORDS. The
/// character before the run must be alphanumeric, which excludes every label ending in `:` and every
/// run that begins a literal after the opening quote. The character after must not be a `{`, which
/// excludes an aligned value placeholder.
///
/// Checked against the two real instances: `over the                limit for bundle metadata` — a
/// letter before, a letter after — and core's `to call                      'devbot.signed.p1:echo'`,
/// where the character after is an apostrophe. Both flag; all nine deliberate alignments do not.
fn has_internal_run(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b' ' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i - start < RUN || start == 0 || i >= bytes.len() {
            continue;
        }
        let before = bytes[start - 1];
        let after = bytes[i];
        // `{` excludes an aligned value placeholder; `=` excludes an aligned SQL assignment, which is
        // the only other place this codebase pads on purpose. Both are shapes a SENTENCE never has
        // after a gap, so excluding them narrows the guard without weakening it -- and the registry's
        // `updated_at    = datetime('now')` is why the second one is here.
        if before.is_ascii_alphanumeric() && after != b'{' && after != b'=' {
            return true;
        }
    }
    false
}

fn rust_sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("reading the source tree").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_user_facing_string_carries_a_run_of_spaces_from_a_lost_line_continuation() {
    let mut files = Vec::new();
    rust_sources(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path(), &mut files);
    assert!(files.len() > 10, "the scan found only {} files; it is not reading the tree", files.len());

    let mut offenders = Vec::new();
    for file in &files {
        // This file describes the defect and would otherwise report itself.
        if file.file_name().is_some_and(|n| n == "message_shape.test.rs") {
            continue;
        }
        let text = std::fs::read_to_string(file).expect("reading a source file");
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            // Comments and doc comments may align things on purpose, and none of them is a message.
            if trimmed.starts_with("//") {
                continue;
            }
            // Only lines that carry a string literal at all.
            if !line.contains('"') {
                continue;
            }
            if has_internal_run(line) {
                offenders.push(format!("{}:{}\n      {}", file.display(), n + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a string literal carries a run of {RUN}+ spaces, which is what a LOST line-continuation \
         backslash looks like. The source reads as a deliberate line break and the message ships with \
         the indentation in it:\n\n  {}\n",
        offenders.join("\n  ")
    );
}

#[test]
fn the_guard_detects_the_shape_it_exists_for() {
    // A GUARD THAT CANNOT FAIL IS A GUARD NOBODY NOTICED STOPPED WORKING. The pattern is built here
    // rather than written literally, so the scan above does not report this test as an offender.
    let gap = " ".repeat(RUN);
    // The two real instances: one from this tree's registry, one core shipped inside a security
    // refusal. A letter before in both; a letter after in one, an apostrophe in the other.
    assert!(has_internal_run(&format!("over the{gap}limit for bundle metadata")));
    assert!(has_internal_run(&format!("to call{gap}'devbot.signed.p1:echo'")));

    // And the shapes it must NOT flag — every one of these is a real line this crate prints, and the
    // first version of the guard reported all of them.
    assert!(!has_internal_run("        let indented = source_code();"), "leading indentation is not a gap");
    assert!(!has_internal_run(&format!("  publishing:{gap} {{}}")), "an aligned label is deliberate");
    assert!(!has_internal_run(&format!("    {{}}")), "an indented list item is deliberate");
    assert!(!has_internal_run("  Run: waffler login --registry https://r.example"), "a two-space indent is deliberate");
    assert!(!has_internal_run("  * every node that installs such a package PINS this publisher;"));
    assert!(!has_internal_run(&format!("trailing spaces are not a gap either{gap}")), "a run at the end has nothing after it");
    assert!(!has_internal_run(&format!("updated_at{gap}= datetime('now')")), "aligned SQL is deliberate");
}
