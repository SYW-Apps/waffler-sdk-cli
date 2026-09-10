#![allow(non_snake_case)]
// Emphatic capitals in a test name are this codebase convention: a name that says WHAT IS
// BEING PROVEN reads better than one that obeys a lint, and a warning nobody clears becomes a
// warning nobody reads - which is how a real one gets missed.

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

#[test]
fn an_end_record_signature_in_the_LAST_21_BYTES_does_not_panic() {
    // CORE FOUND THIS BY MUTATION, IN THEIR PARSER, AND IT IS WORTH HAVING ON THIS SIDE TOO.
    //
    // The scan walks to `len - 4`, so those four bytes can sit close enough to the end that fewer than
    // the 22 a fixed record needs remain — and the very next read is at `i + 20`. Out of bounds.
    //
    // It matters more than a bounds check usually would: this parser's entire input is
    // attacker-supplied and it runs BEFORE any signature is verified, so a panic here is a node
    // crashed by an unsigned file anyone can upload. Their mutation of the length check broke no test,
    // because no fixture had ever put those bytes that close to the end.
    //
    // A REFUSAL OR A CLEAN ANSWER ARE BOTH FINE. What is asserted is that it RETURNS — the failure this
    // guards against is not a wrong verdict, it is no verdict at all.
    let base = archive(b"");
    for trailing in 0..=21usize {
        let mut bytes = base.clone();
        bytes.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        bytes.extend_from_slice(&vec![0xAB; trailing]);
        let _ = detect_framing(&bytes);
    }

    // And the same four bytes as the ENTIRE input, at every length a scan could reach into.
    for len in 4..=25usize {
        let mut bytes = vec![0x50, 0x4b, 0x05, 0x06];
        bytes.extend_from_slice(&vec![0u8; len - 4]);
        let _ = detect_framing(&bytes);
    }
}

// ---------------------------------------------------------------------------------------------
// The producer
// ---------------------------------------------------------------------------------------------

fn signed_bundle(dir: &std::path::Path, seed: &[u8; 32]) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("bundle.zip");
    std::fs::write(&path, archive(b"")).unwrap();
    assert_eq!(sign_as_publisher(&path, seed).unwrap(), Framing::Dual);
    path
}

#[test]
fn a_signed_bundle_is_the_UNCHANGED_archive_with_a_trailer_appended() {
    let dir = tempfile::tempdir().unwrap();
    let before = archive(b"");
    let path = dir.path().join("bundle.zip");
    std::fs::write(&path, &before).unwrap();

    sign_as_publisher(&path, &[9u8; 32]).unwrap();
    let after = std::fs::read(&path).unwrap();

    // THE PAYLOAD IS BYTE-IDENTICAL. The signature covers it, so anything that re-zipped, re-compressed
    // or normalised it would produce an artifact core refuses as an integrity failure — on a stranger's
    // machine, days later, naming nothing about this function.
    assert_eq!(&after[..before.len()], &before[..], "the archive must not be touched");
    assert!(after.len() > before.len());
}

#[test]
fn what_this_writes_is_what_SHARED_reads_back() {
    // THE INTEROP CHECK, AND THE ONLY ONE THAT MATTERS. A producer verified against its own reader
    // agrees with itself forever; what has to hold is that the crate a NODE parses with decodes these
    // exact bytes. If this fails, every other assertion in this file is a statement about my encoder.
    let dir = tempfile::tempdir().unwrap();
    let seed = [11u8; 32];
    let path = signed_bundle(dir.path(), &seed);
    let bytes = std::fs::read(&path).unwrap();

    let parsed = waffler_shared::parse_bundle_frame(&bytes).unwrap();
    // The payload range SHARED identified, taken before the frame is destructured — asserting against
    // a range this test computed for itself would pass for a signature over the wrong bytes.
    let payload = parsed.payload(&bytes).to_vec();
    let waffler_shared::BundleFrame::Signed(trailer) = parsed.frame else {
        panic!("shared did not read a signature trailer");
    };

    assert_eq!(trailer.signatures.len(), 1, "a publisher signs alone; the registry countersigns later");
    let sig = &trailer.signatures[0];
    // ROLE PUBLISHER, NEVER REGISTRY. A tool that emitted a registry signature would be claiming an
    // authority it does not have, and the chain verifier refuses the arrangement before verifying.
    assert_eq!(sig.role, waffler_shared::SignatureRole::Publisher);

    // The key carried is the one the seed produces — a verifier needs nothing pre-shared to check the
    // maths, so a wrong key here verifies nowhere and names nothing.
    let expected = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    assert_eq!(sig.public_key, expected.to_vec());

    // AND THE SIGNATURE ACTUALLY VERIFIES OVER THE PAYLOAD SHARED IDENTIFIED — not over the file, and
    // not over a range this test computed for itself. Both sides deriving the payload the same way is
    // the property; asserting the bytes without it would pass for a signature over the wrong range.
    use ed25519_dalek::Verifier;
    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&expected).unwrap();
    let signature = ed25519_dalek::Signature::from_slice(&sig.signature).unwrap();
    assert!(verifying.verify(&payload, &signature).is_ok(), "the signature must cover the archive shared identified");
}

#[test]
fn signing_an_already_signed_bundle_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = signed_bundle(dir.path(), &[5u8; 32]);
    let before = std::fs::read(&path).unwrap();

    let e = sign_as_publisher(&path, &[6u8; 32]).unwrap_err().to_string();
    // Re-signing means either re-zipping — destroying the payload the existing signature covers — or
    // appending blindly, and both produce artifacts refused later on somebody else's machine.
    assert!(e.contains("already carries a signature"), "got {e}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "a refused signing must not have written anything");
}

#[test]
fn a_publisher_signed_bundle_may_NOT_be_uploaded_unsigned_to_a_registry() {
    let dir = tempfile::tempdir().unwrap();
    let path = signed_bundle(dir.path(), &[4u8; 32]);
    // The registry signs what it accepts and refuses an artifact that already carries a signature. The
    // publisher path is a countersignature the registry adds on receipt — not something this tool
    // uploads twice.
    assert!(refuse_if_signed(&path).is_err());
}

#[test]
fn two_signings_of_one_archive_with_one_key_are_byte_identical() {
    // Ed25519 is deterministic, so a bundle signed twice from the same source and key is the same
    // artifact. That is what keeps a signed bundle reproducible in the same sense an unsigned one is —
    // and it is worth pinning, because a scheme that added randomness would silently take that away.
    let dir = tempfile::tempdir().unwrap();
    let a = signed_bundle(&dir.path().join("a"), &[13u8; 32]);
    let b = signed_bundle(&dir.path().join("b"), &[13u8; 32]);
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

// ─── The chain vector ─────────────────────────────────────────────────────────────────────────

/// WHAT THE PUBLISHER SIGNATURE COVERS, PINNED — and `shared` does not decide it.
///
/// The frame types and the boundary parser are imported, so the encoding and where the archive ends
/// cannot drift. What is NOT imported is the rule about what a signature is computed OVER: this crate
/// chooses to sign the payload, and the registry chooses to sign `payload || publisher_signature`.
/// Those two choices are the chain, they live in two codebases, and nothing in the trailer's encoding
/// can tell a correct pair from a wrong one.
///
/// The interop vector cannot help: its signature is `64 x 0x02` and signs nothing. Two
/// implementations reproduce it byte for byte while disagreeing about coverage. The disagreement
/// fails closed — a node refuses the bundle — but it surfaces at INSTALL, in front of a user, as an
/// integrity failure, rather than here as "the coverage rule changed".
///
/// Ed25519 is deterministic, so the real chain is pinnable. Same vector as `shared@af7a494` and the
/// registry's own suite.
mod chain_vector {
    use ed25519_dalek::{Signer, SigningKey};

    /// Deliberately NOT a zip — the chain rule is indifferent to what it wraps.
    const PAYLOAD: &[u8] = b"waffler chain vector v1";
    const PUBLISHER_SEED: [u8; 32] = [0x22; 32];

    const PUBLISHER_SIG: &str = "7f5d4f6faca93518e153fb3dc0d586b741afcdead4b49d48ef2c17ebae042b07\
                                 62dc6c1860d5680ee60d3f8c0af796c7e14e754008c655e5eb474a3cf826700d";

    #[test]
    fn the_publisher_signature_covers_the_payload_and_nothing_else() {
        let sig = SigningKey::from_bytes(&PUBLISHER_SEED).sign(PAYLOAD);
        assert_eq!(
            hex::encode(sig.to_bytes()),
            PUBLISHER_SIG.replace(char::is_whitespace, ""),
            "what a publisher signature covers drifted from the rule the registry countersigns against"
        );
    }

    #[test]
    fn signing_MORE_than_the_payload_does_not_produce_the_pinned_signature() {
        // THE NEGATIVE HALF. The plausible drift here is including the trailer's own bytes, or the
        // whole file rather than the archive — either would still verify against itself, and the
        // registry's countersignature would then bind a message this crate never signed.
        let mut extended = PAYLOAD.to_vec();
        extended.extend_from_slice(b"and a little more");
        let sig = SigningKey::from_bytes(&PUBLISHER_SEED).sign(&extended);
        assert_ne!(hex::encode(sig.to_bytes()), PUBLISHER_SIG.replace(char::is_whitespace, ""));
    }

    #[test]
    fn the_signer_this_crate_ships_implements_that_rule() {
        // THE POINT OF THE TWO ABOVE. They pin the rule; this proves `sign_as_publisher` implements it
        // rather than that Ed25519 works. It signs a real archive — the vector's payload is not one,
        // and this function locates the payload through the boundary parser — then checks the
        // signature it produced is exactly a signature over the ARCHIVE BYTES, whole and unmodified.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.zip");
        let archive_bytes = super::archive(b"");
        std::fs::write(&path, &archive_bytes).unwrap();

        super::sign_as_publisher(&path, &PUBLISHER_SEED).unwrap();
        let framed = std::fs::read(&path).unwrap();

        let declared =
            u32::from_le_bytes(framed[framed.len() - 12..framed.len() - 8].try_into().unwrap()) as usize;
        let body = &framed[framed.len() - 12 - declared..framed.len() - 12];
        let trailer: waffler_shared::SignatureTrailer = rmp_serde::from_slice(body).unwrap();

        let expected = SigningKey::from_bytes(&PUBLISHER_SEED).sign(&archive_bytes);
        assert_eq!(
            trailer.signatures[0].signature,
            expected.to_bytes().to_vec(),
            "this crate signs something other than the archive bytes"
        );
    }

    #[test]
    fn an_UNROTATED_bundle_writes_no_lineage_key_at_all() {
        // BYTE-IDENTITY IS THE PROPERTY, not "the field is None".
        //
        // `SignatureTrailer` gained a `lineage` field when core built key rotation, and it carries
        // `skip_serializing_if = "Option::is_none"` precisely so a bundle with no rotation chain
        // encodes exactly as it did before the field existed. Without the skip, `to_vec_named`
        // writes `lineage: nil` into EVERY trailer this tool produces and the bytes that two
        // implementations have a byte-for-byte agreement about change silently.
        //
        // Core's interop vector catches it on their side. This crate is the PRODUCER, so it should
        // not learn about its own output from somebody else's test — and a `lineage: None` in the
        // constructor is not evidence of anything until something reads the bytes.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.zip");
        std::fs::write(&path, super::archive(b"")).unwrap();
        super::sign_as_publisher(&path, &PUBLISHER_SEED).unwrap();
        let framed = std::fs::read(&path).unwrap();

        let declared =
            u32::from_le_bytes(framed[framed.len() - 12..framed.len() - 8].try_into().unwrap()) as usize;
        let body = &framed[framed.len() - 12 - declared..framed.len() - 12];

        // THE KEYS, read without the typed struct — which would decode a present `lineage: nil` and
        // an absent key to the same `None`, the one-value-for-two-facts shape this assertion exists
        // to see through. `IgnoredAny` captures the key set without needing a msgpack value crate.
        let decoded: std::collections::BTreeMap<String, serde::de::IgnoredAny> =
            rmp_serde::from_slice(body).expect("the trailer body is a named msgpack map");
        let keys: Vec<&str> = decoded.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["signatures"],
            "an unrotated bundle's trailer must carry ONLY `signatures`; a `lineage` key here \
             changes bytes that core's interop vector pins"
        );
    }
}
