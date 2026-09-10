//! Adversarial tests for the frame boundary, now pointed at `shared`'s parser.
//!
//! ## WHY THESE SURVIVED THE DELETION OF THE CODE THEY WERE WRITTEN FOR
//!
//! The parser moved into `waffler_shared` so this tool, the registry and a node cannot disagree about
//! where an archive ends. The tests did NOT move with it, deliberately: a wire rule two programs
//! depend on is worth being checked by both of their suites, written independently. If core changes
//! the boundary rule, this suite fails here — which is the point, because a rule that only its own
//! author tests is a rule whose next revision nobody else notices.
//!
//! THE HOSTILE CASES ARE THE VALUE. A detector that only ever sees archives this tool wrote will agree
//! with itself forever; what has to hold is that a file built to make two readers disagree about where
//! the archive ends is REFUSED rather than reinterpreted, because that disagreement is the whole
//! exploit.

use super::*;
use std::io::Write;

/// A minimal real archive, so every plant below is grafted onto something valid.
fn archive(comment: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file(".manifest", options).unwrap();
    zip.write_all(br#"{"fqid":"syw.probe.echo","version":"1.0.0"}"#).unwrap();
    if !comment.is_empty() {
        // `set_raw_comment` rather than `set_comment`: THE COMMENT CARRIES ARBITRARY BYTES in these
        // tests, and a String-typed setter runs them through UTF-8 replacement — 0xFF becomes U+FFFD,
        // three bytes where one was planted, and every offset afterwards is wrong. A plant that lands
        // in the wrong place produces a PASSING test that proves nothing.
        zip.set_raw_comment(comment.to_vec().into_boxed_slice());
    }
    zip.finish().unwrap().into_inner()
}

/// The trailer magic, taken from the crate that owns the format rather than restated here.
fn magic() -> [u8; 8] {
    waffler_shared::SIGNATURE_TRAILER_MAGIC
}

#[test]
fn an_archive_this_tool_wrote_reads_as_unsigned() {
    // The shape a registry publish requires, because on that path the registry signs alone.
    assert_eq!(detect_framing(&archive(b"")).unwrap(), Framing::Unsigned);
}

#[test]
fn a_sixty_four_byte_suffix_reads_as_a_legacy_detached_signature() {
    let mut bytes = archive(b"");
    bytes.extend_from_slice(&[0x11; TRAILER_LEN]);
    assert_eq!(detect_framing(&bytes).unwrap(), Framing::Legacy);
}

#[test]
fn the_magic_alone_is_not_enough_because_content_can_contain_it() {
    let mut bytes = archive(b"");
    // A suffix that ends with the magic but is not a frame. Without the length cross-check, ANY
    // content ending in these eight bytes would be read as a signature trailer and the payload
    // boundary would move.
    bytes.extend_from_slice(&magic());
    assert!(detect_framing(&bytes).is_err(), "an 8-byte suffix is neither frame");
}

#[test]
fn a_suffix_matching_no_known_frame_is_REFUSED_rather_than_trimmed() {
    let mut bytes = archive(b"");
    bytes.extend_from_slice(&[0x33; 7]);
    // Deciding by guess which trailing bytes are signature and which are content publishes a corrupt
    // package SUCCESSFULLY, which is the worst outcome available. Refused — that much holds.
    //
    // THE DIAGNOSIS IS WRONG, AND THE ASSERTION IS DELIBERATELY WEAK BECAUSE OF IT. `shared` says
    // "this is not a readable ZIP archive", and it is one: the archive parses, and what is wrong is
    // the seven bytes after it. The cause is that a candidate now qualifies only if its suffix already
    // classifies, which makes the parser's own "N bytes follow the archive, which is neither..."
    // message unreachable — the good diagnostic is dead code and the surviving one describes the
    // wrong thing. Raised with core rather than worked around; asserting the misleading wording here
    // would pin it.
    assert!(detect_framing(&bytes).is_err());
}

#[test]
fn appending_a_second_end_record_moves_the_boundary_rather_than_creating_ambiguity() {
    // This test asserted a REFUSAL and was wrong about why. Under suffix-aware qualification a bare
    // appended copy of the end record is not ambiguous at all: the real record's suffix is now those
    // 22 bytes, which classify as nothing, so it stops qualifying — and the copy, whose suffix is
    // empty, is the only candidate left.
    //
    // The result is Unsigned over the WHOLE file, and that is sound rather than merely tolerable: both
    // readers run this same function, so they agree on the boundary, and the appended bytes become
    // part of what gets signed. Content that is signed is content.
    let base = archive(b"");
    let mut appended = base.clone();
    appended.extend_from_slice(&base[base.len() - 22..]);

    assert_eq!(detect_framing(&appended).unwrap(), Framing::Unsigned);
}

#[test]
fn a_genuinely_ambiguous_boundary_is_refused_and_that_branch_is_REACHABLE() {
    // WORTH ASSERTING PRECISELY BECAUSE THE PREVIOUS TEST STOPPED REACHING IT. Once a candidate has to
    // have a classifying suffix, most two-record files resolve to one answer — which raises a real
    // question about whether the ambiguity refusal is dead code. It is not, and this is the shape that
    // reaches it.
    //
    // Two candidates, both structurally valid, both with classifying suffixes:
    //   the real end record, whose suffix is exactly 64 bytes  -> reads as a legacy signature;
    //   a copy 22 bytes from the end, whose suffix is empty    -> reads as unsigned.
    //
    // So the file says both "an archive plus a signature" and "a longer archive with nothing after
    // it", and the bytes do not decide between them. Choosing would let the author decide where the
    // signed payload ends, which is the entire attack.
    let base = archive(b"");
    let eocd = base[base.len() - 22..].to_vec();
    let mut ambiguous = base.clone();
    ambiguous.extend_from_slice(&[0x55; 64 - 22]); // filler, so the real record's suffix is 64 total
    ambiguous.extend_from_slice(&eocd);

    let e = detect_framing(&ambiguous).unwrap_err().to_string().to_lowercase();
    assert!(
        e.contains("not decidable") || e.contains("refus"),
        "two qualifying candidates must be refused, not resolved by preference: {e}"
    );
}

#[test]
fn a_planted_end_of_central_directory_record_inside_a_comment_does_not_qualify() {
    // A record-shaped 22 bytes hidden in the comment. It has the right signature, so a naive backwards
    // scan that stopped at the FIRST match would take it as the archive's end — and the bytes after it
    // as a signature.
    let mut plant = vec![0x50, 0x4b, 0x05, 0x06];
    plant.extend_from_slice(&[0u8; 18]);
    let bytes = archive(&plant);

    // It must read as UNSIGNED: the plant does not qualify (its zeroed record claims no entries and a
    // zero-length directory at offset 0, which is only legitimate when the record itself sits at
    // offset 0 — and a plant inside a comment never does), so the real record is the single
    // qualifying one.
    assert_eq!(
        detect_framing(&bytes).unwrap(),
        Framing::Unsigned,
        "the plant must not qualify, and the REAL record must still be found"
    );

    // AND THE SECOND HALF OF THE ASSERTION, which is what caught an earlier version of this fix being
    // wrong: the boundary found must be the whole file, not the plant's position. If the plant had
    // qualified, TWO candidates would qualify and detection would refuse — passing the check above for
    // entirely the wrong reason.
    let mut with_suffix = bytes.clone();
    with_suffix.extend_from_slice(&[0x44; TRAILER_LEN]);
    assert_eq!(
        detect_framing(&with_suffix).unwrap(),
        Framing::Legacy,
        "the archive end must be the real record's, so a 64-byte suffix after it reads as a signature"
    );
}


#[test]
fn bytes_that_are_not_an_archive_at_all_are_refused() {
    // The producer holds the file and is deciding whether to upload it. A file whose end cannot be
    // established is one this tool cannot promise carries no signature.
    assert!(detect_framing(b"this is not a zip file, not even close").is_err());
    // Shorter than a record, too — a separate branch in the parser, and one a longer non-archive
    // would not reach.
    assert!(detect_framing(b"tiny").is_err());
}

#[test]
fn the_file_reader_and_the_byte_reader_agree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bundle.zip");
    let bytes = archive(b"");
    std::fs::write(&path, &bytes).unwrap();
    // Two entry points, one answer. A convenience wrapper that read a different range would be a
    // second boundary parser, which is the exact class of defect this module was rewritten to remove.
    assert_eq!(detect_framing_of_file(&path).unwrap(), detect_framing(&bytes).unwrap());
}

#[test]
fn an_unsigned_bundle_may_be_published_and_a_signed_one_may_not() {
    let dir = tempfile::tempdir().unwrap();

    let unsigned = dir.path().join("unsigned.zip");
    std::fs::write(&unsigned, archive(b"")).unwrap();
    assert_eq!(refuse_if_signed(&unsigned).unwrap(), Framing::Unsigned);

    // BOTH HALVES. A refusal test alone would pass for a function that refused everything, which is
    // the failure a developer reads as the tool being broken rather than their bundle.
    let signed = dir.path().join("signed.zip");
    let mut bytes = archive(b"");
    bytes.extend_from_slice(&[0x11; TRAILER_LEN]);
    std::fs::write(&signed, bytes).unwrap();
    let e = refuse_if_signed(&signed).unwrap_err().to_string();
    assert!(e.contains("already"), "the refusal must say what is wrong with the artifact: {e}");
    assert!(e.contains("unsigned bundle"), "and what to do instead: {e}");
}
