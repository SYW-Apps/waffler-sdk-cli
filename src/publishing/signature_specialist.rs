//! `sdk_cli::signature-specialist` — what follows the archive.
//!
//! Spec: `sdk_cli::signature-specialist` / `isignature-specialist` / `signature_specialist_impl`.
//!
//! ## THIS IS DETECTION ONLY, AND THE MISSING HALF IS A DECISION
//!
//! The spec designed a `signAsPublisher` alongside this. It is not here, and the reason is stronger
//! than "not yet":
//!
//! The frame already has a producer and a reader in `registry::publication::bundle_framing` — `frame`
//! writes it, `read_framing` reads it — and `waffler_core` needs a reader of its own. A publisher
//! signer here would be the THIRD implementation of one wire format, and two of the three would be
//! unexercised on any given day. A wire format with three definitions is a format that drifts in
//! whichever copy nobody runs, and the drift is silent on both sides until a bundle crosses.
//!
//! So when the producer half is needed, the frame types belong in ONE crate both sides depend on
//! (`shared`), not re-declared here. That is a larger change than this tool, and doing the small
//! wrong version first is how the larger one stops being possible.
//!
//! What ships now is what is needed now: refusing to upload a bundle whose trailing bytes are not the
//! shape a registry publish accepts.
//!
//! ## THE ALGORITHM IS MIRRORED, NOT INVENTED
//!
//! `archive_end_offset` below is the same comment-aware, central-directory-validated, ambiguity-
//! refusing scan the registry runs. Two readers picking different ends of one file is the entire
//! attack this validation closes: the bundle a developer inspects and the bundle a registry validates
//! would differ, and neither could report it.

use anyhow::{bail, Result};

use super::types::Framing;

/// The detached signature length core's verifier reads unconditionally.
pub const TRAILER_LEN: usize = 64;

/// The magic identifying a dual-signature trailer. The LAST eight bytes, so detection is a
/// fixed-offset read with no parsing.
pub const MAGIC: [u8; 8] = *b"WFLRSIG\x01";

/// `trailer_len: u32 LE` + `magic: [u8; 8]`.
const FRAME_FOOTER_LEN: usize = 12;

/// Decide what the bytes after the archive are.
pub fn detect_framing(artifact: &[u8]) -> Result<Framing> {
    // STRICTER THAN THE REGISTRY HERE, ON PURPOSE, AND IN THE SAFE DIRECTION.
    //
    // The registry treats "no single coherent archive end" as "not an archive" and carries on to its
    // reader — which is sound THERE, because on its unsigned path the payload it signs is the whole
    // artifact, so its own boundary opinion never has to match anyone else's.
    //
    // This is the producer, holding the file, deciding whether to upload it. A file whose end cannot
    // be established is one this tool cannot promise carries no signature, and refusing costs a
    // developer one clear local message while accepting costs an artifact nobody can install.
    let Some(end) = archive_end_offset(artifact) else {
        bail!(
            "no single coherent archive end could be found in these {} bytes, so whether anything is \
             appended to them cannot be established. Refusing rather than guessing: guessing decides \
             where a signature begins.",
            artifact.len()
        );
    };
    let suffix = &artifact[end..];

    if suffix.is_empty() {
        // The shape a registry publish requires, because on that path the registry signs alone.
        return Ok(Framing::Unsigned);
    }

    if suffix.len() >= FRAME_FOOTER_LEN && suffix[suffix.len() - 8..] == MAGIC {
        let len_at = suffix.len() - FRAME_FOOTER_LEN;
        let declared = u32::from_le_bytes([suffix[len_at], suffix[len_at + 1], suffix[len_at + 2], suffix[len_at + 3]]) as usize;
        // THE CROSS-CHECK. The archive says where it ends; the trailer says how long it is. Requiring
        // them to agree removes the unbounded-length question by arithmetic — and a magic ALONE is a
        // byte pattern content can contain, so only the agreement makes it a frame rather than a
        // coincidence.
        if declared + FRAME_FOOTER_LEN != suffix.len() {
            bail!(
                "a signature trailer declares {declared} bytes but {} follow the archive; the archive's own end and the trailer's declared length disagree",
                suffix.len() - FRAME_FOOTER_LEN
            );
        }
        return Ok(Framing::Dual);
    }

    if suffix.len() == TRAILER_LEN {
        return Ok(Framing::Legacy);
    }

    // REFUSED, NAMING THE SUFFIX LENGTH. Trailing bytes matching no known frame are not trimmed and
    // not ignored: deciding by guess which trailing bytes are signature and which are content
    // publishes a corrupt package SUCCESSFULLY, which is the worst outcome available.
    bail!(
        "{} bytes follow the archive, which is neither a {TRAILER_LEN}-byte detached signature nor a \
         dual-signature trailer; refusing rather than guessing where the payload ends",
        suffix.len()
    )
}

/// Where the zip archive ends, or `None` when that cannot be established.
///
/// MIRRORED FROM `registry::publication::bundle_framing::archive_end_offset`. Every clause here
/// exists because of something that got through a simpler version of it:
///
///   * the scan is COMMENT-AWARE, because a zip comment may contain bytes that look like an
///     end-of-central-directory record, so several candidates appear in a hostile file;
///   * a candidate qualifies only when its central-directory pointer ADDRESSES A REAL DIRECTORY,
///     because a planted record can otherwise claim any end it likes;
///   * an archive declaring zero entries qualifies only when its record sits at offset zero, because
///     a zeroed plant declares "no entries, zero-length directory at offset 0" and otherwise reads as
///     a legitimate empty archive;
///   * two qualifying candidates is a REFUSAL, because which one is "the" boundary is not a question
///     the bytes answer.
fn archive_end_offset(bytes: &[u8]) -> Option<usize> {
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const CD_HEADER_SIG: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
    const EOCD_MIN: usize = 22;
    const MAX_SCAN: usize = EOCD_MIN + u16::MAX as usize;

    if bytes.len() < EOCD_MIN {
        return None;
    }
    let scan_floor = bytes.len().saturating_sub(MAX_SCAN + TRAILER_LEN);
    let mut qualified: Option<usize> = None;
    let mut i = bytes.len() - EOCD_MIN;
    loop {
        if bytes[i..i + 4] == EOCD_SIG {
            let comment_len = u16::from_le_bytes([bytes[i + 20], bytes[i + 21]]) as usize;
            let end = i + EOCD_MIN + comment_len;
            let entries = u16::from_le_bytes([bytes[i + 10], bytes[i + 11]]) as usize;
            let cd_size = u32::from_le_bytes([bytes[i + 12], bytes[i + 13], bytes[i + 14], bytes[i + 15]]) as usize;
            let cd_offset = u32::from_le_bytes([bytes[i + 16], bytes[i + 17], bytes[i + 18], bytes[i + 19]]) as usize;

            let directory_is_real = match cd_offset.checked_add(cd_size) {
                // The directory must lie wholly before this record, and start with a central-directory
                // file header. `checked_add` rather than `+`: both halves are attacker-supplied u32s
                // and their sum overflows usize on a 32-bit target, where a wrap would make a bogus
                // pointer pass the bounds test.
                Some(cd_end) if cd_end <= i => {
                    if entries == 0 {
                        // A real empty zip is exactly its 22-byte record, so it starts at offset 0. A
                        // zeroed record planted inside a comment never does.
                        i == 0
                    } else {
                        cd_size >= 4 && bytes[cd_offset..cd_offset + 4] == CD_HEADER_SIG
                    }
                }
                _ => false,
            };

            if end <= bytes.len() && directory_is_real {
                if qualified.is_some() {
                    return None;
                }
                qualified = Some(end);
            }
        }
        if i == 0 || i <= scan_floor {
            return qualified;
        }
        i -= 1;
    }
}

/// Read a bundle from disk and decide its framing.
pub fn detect_framing_of_file(path: &std::path::Path) -> Result<Framing> {
    // READ WHOLE, WHICH IS THE ONE PLACE THAT IS ACCEPTABLE. The scan runs backwards from the end
    // over up to 64 KiB plus a trailer, so a streaming version would need a seek-and-read window —
    // and this runs once per publish on a file the developer just produced, against an upload that
    // will read the same bytes again anyway. If bundles outgrow memory this is the function to
    // change, and the boundary logic above does not have to move with it.
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("reading {} to check what is appended to it: {e}", path.display()))?;
    detect_framing(&bytes)
}

#[cfg(test)]
#[path = "signature_specialist.test.rs"]
mod tests;
