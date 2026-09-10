//! `sdk_cli::signature-specialist` — what follows the archive.
//!
//! Spec: `sdk_cli::signature-specialist` / `isignature-specialist` / `signature_specialist_impl`.
//!
//! ## THE BOUNDARY RULE IS `shared`'s, AND THIS NO LONGER RESTATES IT
//!
//! It used to. This module carried its own magic, its own trailer lengths, and its own
//! comment-aware, central-directory-validated, ambiguity-refusing scan for the end of the archive —
//! mirrored from the registry's, which had mirrored it from nowhere because it was the first.
//!
//! That was two statements of one rule, and the module docs said so at the time: the fix was to move
//! the frame into ONE crate both sides depend on rather than to add a third copy. `waffler_core` has
//! now done exactly that, so this calls [`waffler_shared::parse_bundle_frame`] and the duplicate is
//! deleted. Two readers picking different ends of one file is the entire attack the validation
//! exists to close, and the only way to be sure two readers agree is for there to be one reader.
//!
//! WHAT IS KEPT IS THE TESTS. They are adversarial — a record-shaped plant inside a zip comment, a
//! second record that also describes a coherent archive, a magic with a lying length — and pointing
//! them at the shared parser gives that parser a second, independently written suite. A wire rule
//! two programs depend on is worth being checked by both of their test suites.
//!
//! ## THIS IS DETECTION ONLY
//!
//! There is no `signAsPublisher` here. The producer half is gated on core's verifiers being deployed
//! — emitting a signature shape deployed readers cannot verify produces artifacts nobody can install
//! — and when it lands it will use `shared`'s trailer types for the same reason this parser now does.

use anyhow::{bail, Result};

use super::types::Framing;

/// The detached signature length core's verifier reads unconditionally.
pub use waffler_shared::LEGACY_SIGNATURE_LEN as TRAILER_LEN;

/// Decide what the bytes after the archive are.
///
/// STRICTER THAN A PUBLISH NEEDS TO BE, ON PURPOSE AND IN THE SAFE DIRECTION. A file whose archive
/// end cannot be established — no valid record, or two that both qualify — is one this tool cannot
/// promise carries no signature, and refusing costs a developer one clear local message while
/// accepting costs an artifact nobody can install.
pub fn detect_framing(artifact: &[u8]) -> Result<Framing> {
    let parsed = waffler_shared::parse_bundle_frame(artifact)
        .map_err(|e| anyhow::anyhow!("{}", e.message))?;

    Ok(match parsed.frame {
        // The shape a registry publish requires, because on that path the registry signs alone.
        waffler_shared::BundleFrame::Unsigned => Framing::Unsigned,
        waffler_shared::BundleFrame::Legacy => Framing::Legacy,
        waffler_shared::BundleFrame::Signed(_) => Framing::Dual,
    })
}

/// Read a bundle from disk and decide its framing.
pub fn detect_framing_of_file(path: &std::path::Path) -> Result<Framing> {
    // READ WHOLE, WHICH IS THE ONE PLACE THAT IS ACCEPTABLE. The scan runs backwards from the end
    // over a bounded window, so a streaming version would need a seek-and-read window — and this runs
    // once per publish on a file the developer just produced, against an upload that will read the
    // same bytes again anyway. If bundles outgrow memory this is the function to change, and the
    // boundary logic does not have to move with it because it is no longer here.
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("reading {} to check what is appended to it: {e}", path.display()))?;
    detect_framing(&bytes)
}

/// Whether a bundle may be uploaded to a registry that signs what it accepts.
///
/// A SEPARATE FUNCTION SO THE REASON TRAVELS WITH THE ANSWER. The registry refuses an artifact that
/// already carries a signature — it cannot safely decide which trailing bytes are signature and which
/// are content — so a caller needs the refusal's wording as much as the verdict.
pub fn refuse_if_signed(path: &std::path::Path) -> Result<Framing> {
    let framing = detect_framing_of_file(path)?;
    if framing != Framing::Unsigned {
        bail!(
            "{} is already {framing}, and the registry signs what it accepts: it refuses an artifact \
             that already carries a signature, because it cannot safely decide which trailing bytes \
             are signature and which are content.\n  Pack an unsigned bundle for a registry publish.",
            path.display()
        );
    }
    Ok(framing)
}

#[cfg(test)]
#[path = "signature_specialist.test.rs"]
mod tests;
