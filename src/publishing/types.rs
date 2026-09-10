//! The publishing subsystem's data model.
//!
//! Spec: `sdk_cli::publishing` types — `written-bundle`, `publish-outcome`.

/// What follows the archive in a bundle file.
///
/// THE FRAMING DECIDES WHERE A BUNDLE MAY GO. `Unsigned` is the shape a registry publish requires,
/// since the registry signs alone on that path and refuses an upload that already carries a registry
/// signature — it cannot safely decide which trailing bytes are signature and which are content, and
/// guessing wrong publishes a corrupt package SUCCESSFULLY, which is the worst outcome available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// Nothing after the archive. What a registry publish requires.
    Unsigned,
    /// A single detached signature appended to the payload — the original format.
    Legacy,
    /// A signature trailer carrying one or more signatures with a length and a magic.
    Dual,
}

impl std::fmt::Display for Framing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unsigned => "unsigned",
            Self::Legacy => "signed (detached)",
            Self::Dual => "signed (trailer)",
        })
    }
}

/// A bundle that exists on disk.
///
/// ONCE WRITTEN IT IS OPAQUE. Nothing downstream may re-zip, re-compress or normalise it — a
/// transformation that looks harmless invalidates a signature over the whole payload and produces an
/// artifact core refuses as IntegrityFailure, discovered on a stranger's machine rather than here.
#[derive(Debug, Clone)]
pub struct WrittenBundle {
    /// Kept after a failed upload rather than cleaned up: the developer can inspect it and the next
    /// attempt does not rebuild.
    pub path: std::path::PathBuf,
    pub fqid: String,
    pub version: String,
    pub framing: Framing,
    pub size_bytes: u64,
}

/// What the registry answered, and where the artifact is regardless.
#[derive(Debug, Clone)]
pub struct PublishOutcome {
    /// The normalised base URL the bundle was actually sent to.
    ///
    /// NAMED, ALWAYS. A publish that quietly went to the wrong registry is the failure an operator
    /// discovers last and trusts least, and this is the record that makes it discoverable at all.
    pub registry: String,
    pub fqid: String,
    pub version: String,
    /// Present on success and on failure alike, so a developer wanting to inspect what was accepted
    /// need not re-derive the path.
    pub bundle_path: std::path::PathBuf,
    /// The registry's own answer, carried through rather than reinterpreted.
    pub receipt: serde_json::Value,
}

/// What a registry already holds for one package.
#[derive(Debug, Clone, Default)]
pub struct PublishedPackage {
    pub versions: Vec<PublishedVersion>,
}

impl PublishedPackage {
    /// Has this package ever carried a publisher signature?
    ///
    /// THE REGISTRY'S FACT, NOT A LOCAL ONE. Assuming it from local state would let a fresh clone
    /// downgrade a package silently, which is the whole attack the refusal exists to stop.
    pub fn has_publisher_signature(&self) -> bool {
        self.versions.iter().any(|v| v.publisher_signed)
    }
}

#[derive(Debug, Clone)]
pub struct PublishedVersion {
    pub version: String,
    pub publisher_signed: bool,
}
