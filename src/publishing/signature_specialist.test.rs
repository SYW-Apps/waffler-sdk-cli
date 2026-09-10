//! Tests for the framing detector.
//!
//! THE HOSTILE CASES ARE THE POINT. A detector that only ever sees archives this tool wrote will agree
//! with itself forever; what has to hold is that a file built to make two readers disagree about where
//! the archive ends is REFUSED rather than reinterpreted, because that disagreement is the whole exploit.

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
fn a_trailer_reads_as_dual_only_when_its_declared_length_AGREES_with_the_suffix() {
    let base = archive(b"");

    // A body of 20 bytes, then the length, then the magic.
    let body = vec![0x22u8; 20];
    let mut good = base.clone();
    good.extend_from_slice(&body);
    good.extend_from_slice(&(body.len() as u32).to_le_bytes());
    good.extend_from_slice(&MAGIC);
    assert_eq!(detect_framing(&good).unwrap(), Framing::Dual);

    // THE SAME BYTES WITH A LYING LENGTH. Requiring the archive's own end and the trailer's declared
    // length to agree removes the unbounded-length question by arithmetic, before anything parses
    // attacker-supplied content.
    let mut lying = base.clone();
    lying.extend_from_slice(&body);
    lying.extend_from_slice(&999_u32.to_le_bytes());
    lying.extend_from_slice(&MAGIC);
    let e = detect_framing(&lying).unwrap_err().to_string();
    assert!(e.contains("disagree"), "got {e}");
}

#[test]
fn the_magic_alone_is_not_enough_because_content_can_contain_it() {
    let mut bytes = archive(b"");
    // A suffix that ends with the magic but is not a frame: its declared length cannot agree, so it is
    // refused. Without the length cross-check, ANY content ending in these eight bytes would be read as
    // a signature trailer and the payload boundary would move.
    bytes.extend_from_slice(&MAGIC);
    let e = detect_framing(&bytes).unwrap_err().to_string();
    assert!(e.contains("neither"), "an 8-byte suffix is neither frame: {e}");
}

#[test]
fn a_suffix_matching_no_known_frame_is_REFUSED_rather_than_trimmed() {
    let mut bytes = archive(b"");
    bytes.extend_from_slice(&[0x33; 7]);
    let e = detect_framing(&bytes).unwrap_err().to_string();
    // Deciding by guess which trailing bytes are signature and which are content publishes a corrupt
    // package SUCCESSFULLY, which is the worst outcome available.
    assert!(e.contains('7'), "the message names the suffix length: {e}");
    assert!(e.contains("refusing rather than guessing"), "got {e}");
}

#[test]
fn a_planted_end_of_central_directory_record_inside_a_comment_does_not_qualify() {
    // A record-shaped 22 bytes hidden in the comment. It has the right signature, so a naive
    // backwards scan that stopped at the FIRST match would take it as the archive's end — and the
    // bytes after it as a signature.
    let mut plant = vec![0x50, 0x4b, 0x05, 0x06];
    plant.extend_from_slice(&[0u8; 18]);
    let bytes = archive(&plant);

    // It must read as UNSIGNED: the plant does not qualify (its zeroed record claims no entries and a
    // zero-length directory at offset 0, which is only legitimate when the record itself sits at offset
    // 0 — and a plant inside a comment never does), so the real record is the single qualifying one.
    assert_eq!(
        detect_framing(&bytes).unwrap(),
        Framing::Unsigned,
        "the plant must not qualify, and the REAL record must still be found"
    );

    // AND THE SECOND HALF OF THE ASSERTION, which is what caught the first version of this fix being
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
fn a_plant_that_points_at_a_real_directory_makes_the_boundary_AMBIGUOUS_and_is_refused() {
    // The hard case: a second record that also describes a coherent archive. Which one is "the"
    // boundary is not a question the bytes answer, and guessing decides where a signature begins.
    //
    // Built by taking a real archive and appending a COPY of its own end record. The copy's pointers
    // are still valid, so both qualify.
    let base = archive(b"");
    let real_eocd = &base[base.len() - 22..];
    let mut ambiguous = base.clone();
    ambiguous.extend_from_slice(real_eocd);

    let e = detect_framing(&ambiguous).unwrap_err().to_string();
    assert!(
        e.contains("no single coherent archive end"),
        "two qualifying candidates must be refused, not resolved by preference: {e}"
    );
}

#[test]
fn bytes_that_are_not_an_archive_at_all_are_refused() {
    // The producer holds the file and is deciding whether to upload it. A file whose end cannot be
    // established is one this tool cannot promise carries no signature.
    let e = detect_framing(b"this is not a zip file, not even close").unwrap_err().to_string();
    assert!(e.contains("no single coherent archive end"), "got {e}");
    // Shorter than a record, too.
    assert!(detect_framing(b"tiny").is_err());
}

#[test]
fn the_file_reader_and_the_byte_reader_agree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bundle.zip");
    let bytes = archive(b"");
    std::fs::write(&path, &bytes).unwrap();
    // Two entry points, one answer. A convenience wrapper that read a different range would be a second
    // boundary parser, which is the exact class of defect this module exists to avoid.
    assert_eq!(detect_framing_of_file(&path).unwrap(), detect_framing(&bytes).unwrap());
}
