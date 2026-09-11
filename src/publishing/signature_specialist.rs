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

/// Append a publisher signature to an unsigned archive.
///
/// ## THE PUBLISHER SIGNS FIRST AND THE REGISTRY LAST
///
/// Coverage is STRUCTURAL: `signatures[i]` covers `zip || signatures[0..i].signature`, so the ORDER
/// in the trailer is the countersigning chain. There is deliberately no field saying what a signature
/// covers — a verifier reading one would be letting attacker-supplied data decide what to verify.
///
/// So this only ever produces a trailer holding ONE signature, role Publisher. The registry
/// countersigns on receipt and its signature must be the final element, committing to the publisher's
/// beneath it; reversed, both still verify and the publisher signature becomes swappable. A tool that
/// emitted a Registry signature would be claiming an authority it does not have, and `verify_chain`
/// refuses the arrangement before it verifies anything.
///
/// ## THIS IS A ONE-WAY DOOR FOR THE PACKAGE
///
/// Once an fqid has shipped a publisher signature, every node that installed it pins that publisher.
/// An unsigned publish afterwards is refused as a downgrade, and a DIFFERENT publisher is refused
/// outright because rotation is not built. The caller states that before calling this, not after.
pub fn sign_as_publisher(bundle_path: &std::path::Path, signing_key: &[u8; 32]) -> Result<Framing> {
    let bytes = std::fs::read(bundle_path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", bundle_path.display()))?;

    // THE SAME PARSER THE DETECTION PATH USES, so the producer and the detector cannot disagree about
    // where the archive ends.
    let parsed = waffler_shared::parse_bundle_frame(&bytes).map_err(|e| anyhow::anyhow!("{}", e.message))?;
    if parsed.frame != waffler_shared::BundleFrame::Unsigned {
        bail!(
            "{} already carries a signature. Re-signing means either re-zipping — which destroys the \
             payload the existing signature covers — or appending blindly, and both produce an artifact \
             refused later on somebody else's machine.",
            bundle_path.display()
        );
    }

    // SIGN THE PARSER'S PAYLOAD, not the file length. For an unsigned bundle they are the same, and
    // writing it as the parser's answer is what keeps it true once a publisher signature is followed
    // by a registry one.
    let payload = parsed.payload(&bytes);
    let key = ed25519_dalek::SigningKey::from_bytes(signing_key);
    let signature = ed25519_dalek::Signer::sign(&key, payload);

    // BUILT THROUGH `new`, NOT AS A LITERAL. `SignatureTrailer` is `#[non_exhaustive]`, so a struct
    // expression outside `shared` is refused at compile time — which is deliberate and in this
    // crate's favour. The type's own doc plans more fields (algorithms, timestamps, roles), and a
    // literal here would break this build on every one of them; `new` takes each addition's default
    // instead. It cost one break to stop costing one per field.
    //
    // NO ROTATION CHAIN, because `waffler key rotate` is not built — this tool has never produced a
    // bundle that rotates a publisher key. `new` leaves `lineage` as `None`, and that is the truth
    // rather than a placeholder.
    //
    // IT MUST STAY ABSENT UNTIL ROTATION IS BUILT, and the reason is not tidiness. The field is
    // `skip_serializing_if = "Option::is_none"`, so `None` writes NO KEY into the trailer and the
    // bytes are identical to those produced before the field existed. Any other value — including an
    // EMPTY chain — writes a key, changes the trailer, and breaks the byte-for-byte agreement the
    // interop vectors hold between this tool and core. Asserted in the tests beside this file.
    let trailer = waffler_shared::SignatureTrailer::new(vec![waffler_shared::BundleSignature {
        role: waffler_shared::SignatureRole::Publisher,
        public_key: key.verifying_key().to_bytes().to_vec(),
        signature: signature.to_bytes().to_vec(),
    }]);

    // NAMED MessagePack, so a later addition to the trailer is non-breaking. The encoding, the length
    // and the magic all come from `shared` rather than being restated here.
    let body = rmp_serde::to_vec_named(&trailer)
        .map_err(|e| anyhow::anyhow!("encoding the signature trailer: {e}"))?;

    // BODY, THEN LENGTH, THEN MAGIC — the frame is read back to front, which is why the two
    // self-describing parts come last.
    let mut out = payload.to_vec();
    out.extend_from_slice(&body);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&waffler_shared::SIGNATURE_TRAILER_MAGIC);

    std::fs::write(bundle_path, &out)
        .map_err(|e| anyhow::anyhow!("writing {}: {e}", bundle_path.display()))?;

    // READ BACK THROUGH THE PARSER RATHER THAN ASSERTED. What was written is only right if the reader
    // that matters agrees, and this is the cheapest possible moment to find out that it does not —
    // rather than on a node, months later, as an integrity failure naming nothing.
    detect_framing(&out)
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
