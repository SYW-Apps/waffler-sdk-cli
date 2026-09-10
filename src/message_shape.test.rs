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

/// Is this line one the scan should look at?
///
/// EXTRACTED FROM THE WALKER SO IT CAN BE KILLED. Both halves were inline `continue`s, and
/// neutralising either left the suite green — not because they do nothing, but because nothing could
/// observe them. A filter inside a loop is a decision no test can reach; a named predicate is one
/// that can be handed the shapes it exists to reject.
fn is_scannable(line: &str) -> bool {
    // A comment may align things on purpose and none of them is a message a user sees.
    if line.trim_start().starts_with("//") {
        return false;
    }
    // Only lines carrying a string literal at all. This is also what makes the examined-lines floor
    // mean something: without it the count is "every line in the crate", which a broken filter would
    // still satisfy.
    line.contains(char::from(34))
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

    let mut examined_lines = 0usize;
    let mut scanned_a_nested_file = false;
    let mut offenders = Vec::new();

    for file in &files {
        // THIS FILE IS NOT EXCLUDED, AND THAT IS A CORRECTION.
        //
        // It was, by name — the usual arrangement, because a guard's fixtures are deliberately
        // malformed and the first run reports its own evidence as the defect. The cost is that the
        // guard's OWN messages then go unchecked, and mine had the defect in two of them: the
        // "swallowing the tree" refusal and the offender report both shipped with runs of spaces, in
        // the one file that could not catch them. Found by reading a failing run's output, which is
        // the only place it was ever visible.
        //
        // The exclusion is unnecessary here because every fixture is BUILT AT RUNTIME from `gap`
        // rather than written literally, so no run of spaces appears in this file's source at all.
        // That was already the reason given for constructing them that way; it just also removes the
        // need to opt out.
        //
        // RECURSION, PROVEN BY A FILE THAT COULD ONLY COME FROM A SUBDIRECTORY. A walker that stops at
        // the top level is green and reads a tenth of the tree, and a file COUNT cannot tell the
        // difference once the top level alone clears whatever floor was chosen.
        if file.parent().is_some_and(|p| p.file_name().is_some_and(|n| n != "src")) {
            scanned_a_nested_file = true;
        }

        let text = std::fs::read_to_string(file).expect("reading a source file");
        for (n, line) in text.lines().enumerate() {
            if !is_scannable(line) {
                continue;
            }
            examined_lines += 1;
            if has_internal_run(line) {
                offenders.push(format!("{}:{}
      {}", file.display(), n + 1, line.trim()));
            }
        }
    }

    // ASK WHAT THE CHECK READ, NOT WHAT IT COVERS.
    //
    // Core hit this in their copy: breaking the part that pulls text out of a file left the scan
    // GREEN, because a scan that reads every file in a crate and examines nothing from any of them is
    // indistinguishable from a clean tree. A file count does not catch it — the files were found, they
    // simply contributed nothing.
    //
    // So the floors are on what was actually EXAMINED, and on where it came from. Both are far below
    // the real numbers, so they fail on a broken walker rather than on ordinary growth or deletion.
    assert!(files.len() > 10, "the walker found only {} files; it is not reading the tree", files.len());
    assert!(scanned_a_nested_file, "every scanned file was at the top level; the walker is not recursing");
    assert!(
        examined_lines > 500,
        "only {examined_lines} lines carrying a string literal were examined; the filter is \
         swallowing the tree and this check is green because it measured nothing"
    );

    assert!(
        offenders.is_empty(),
        "a string literal carries a run of {RUN}+ spaces, which is what a LOST line-continuation \
         backslash looks like. The source reads as a deliberate line break, and the message ships \
         with the indentation in it:\n\n  {}\n",
        offenders.join("\n  ")
    );
}

#[test]
fn the_predicate_still_detects_the_shape_it_exists_for() {
    // THE HALF THAT STOPS THIS CHECK DYING QUIETLY. Narrowing the predicate to nothing makes the scan
    // pass, and narrowing-to-silence-a-false-positive is exactly how that happens - it happened once
    // already, when the first version flagged nine deliberate alignments and the obvious response was
    // to loosen it until they stopped.
    //
    // SEPARATE FROM THE NEGATIVE CASES, because the two failures are opposite and want opposite
    // fixes: this one failing means the guard no longer detects, the other failing means it cries
    // wolf. A single test covering both says only "the predicate is wrong".
    //
    // The pattern is built here rather than written literally, so the scan does not report this file.
    let gap = " ".repeat(RUN);
    // The two real instances: one from this tree's registry, one core shipped inside a security
    // refusal. A letter before in both; a letter after in one, an apostrophe in the other.
    assert!(has_internal_run(&format!("over the{gap}limit for bundle metadata")));
    assert!(has_internal_run(&format!("to call{gap}'devbot.signed.p1:echo'")));
}

#[test]
fn the_predicate_does_not_flag_DELIBERATE_alignment() {
    // Every one of these is a real line this crate prints, and the first version of the guard
    // reported all of them. A guard whose failures are all false is one someone deletes - and
    // deleting it would be the correct response, which is the worst way for a check to fail.
    let gap = " ".repeat(RUN);

    // THE CASE THAT COVERS `before.is_ascii_alphanumeric()`, AND IT WAS MISSING. Neutering that clause
    // survived every test: each existing negative had a `{` or an `=` after the run, so the OTHER two
    // exclusions caught them and this one decided nothing. A clause no test can kill is a clause
    // nobody can tell is needed.
    //
    // What it actually excludes is a run immediately after an opening quote — an indented literal —
    // where the character after is an ordinary letter and neither other clause applies.
    assert!(
        !has_internal_run(&format!("println!(\"{gap}indented output\");")),
        "a run right after an opening quote is indentation, not a sentence gap"
    );

    assert!(!has_internal_run("        let indented = source_code();"), "leading indentation is not a gap");
    assert!(!has_internal_run(&format!("  publishing:{gap} {{}}")), "an aligned label is deliberate");
    assert!(!has_internal_run("    {}"), "an indented list item is deliberate");
    assert!(!has_internal_run("  Run: waffler login --registry https://r.example"), "a two-space indent is deliberate");
    assert!(!has_internal_run("  * every node that installs such a package PINS this publisher;"));
    assert!(!has_internal_run(&format!("trailing spaces are not a gap either{gap}")), "a run at the end has nothing after it");
    assert!(!has_internal_run(&format!("updated_at{gap}= datetime('now')")), "aligned SQL is deliberate");

    // THE CASE THAT COVERS `after != b'{'`, AND IT WAS ALSO MISSING. Every other aligned-column case
    // here has a `:` before the run, so `before` excluded them and this clause decided nothing — the
    // same redundancy as `before` had, one clause along.
    //
    // The shape it actually excludes is an aligned column with NO punctuation before the placeholder.
    // Neither crate contains one today, which is why nothing could kill the clause; it is an ordinary
    // Rust line rather than an unreachable one, so the clause is kept and given the case rather than
    // deleted.
    assert!(
        !has_internal_run(&format!("println!(\"Total{gap}{{}}\", n);")),
        "padding before a format placeholder is alignment, not a sentence gap"
    );
}

#[test]
fn the_line_filter_rejects_what_it_exists_to_reject() {
    // BOTH HALVES WERE UNKILLABLE AS INLINE `continue`s. Neutralising either left the suite green,
    // because a decision inside a loop is one no test can hand a value to. Extracting the filter is
    // what made these assertions possible at all.
    let gap = " ".repeat(RUN);

    // A comment may align on purpose, and none of them is a message a user reads. The shape that
    // reaches this clause needs a quote too, or the other half rejects it first — which is exactly
    // why neither could be killed while they were fused together.
    assert!(!is_scannable(&format!("    // see \"over the{gap}limit\" above")));
    assert!(!is_scannable(&format!("//! the message ships as `over the{gap}limit`")));

    // A line with no string literal carries no message, whatever else is on it.
    assert!(!is_scannable(&format!("    let x = y{gap}+ z;")));

    // And the positive: an ordinary line carrying a message is scanned.
    assert!(is_scannable("    bail!(\"the archive declares no fqid\");"));
}
