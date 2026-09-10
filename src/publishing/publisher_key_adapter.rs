//! `sdk_cli::publisher-key-adapter` — where a publisher's signing key comes from.
//!
//! Spec: `sdk_cli::publisher-key-adapter` / `ipublisher-key-adapter` / `publisher_key_adapter_impl`.
//!
//! ## THE KEY IS A ONE-WAY DOOR
//!
//! Once a package has shipped a publisher signature, every node that installed it PINS that
//! publisher. A bundle offering a different one is refused; rotation is not built; and a lost key
//! means a package that can never be updated on any node that already has it.
//!
//! That is not a caveat for a changelog. It is the single most consequential fact about the file this
//! module creates, so creating one states it at the moment of creation and nothing here ever
//! generates a key implicitly.
//!
//! ## A PATH, NEVER A RAW KEY IN THE ENVIRONMENT
//!
//! A private key in an environment variable appears in process listings, in a crashed process's dump,
//! and in the log of any CI system that echoes its environment. A path does not. CI systems already
//! write secrets to files for exactly this reason, so taking a path costs an automated publisher
//! nothing and removes a whole class of leak.

use std::path::Path;

use anyhow::{bail, Context, Result};

/// An Ed25519 seed: 32 bytes, hex-encoded on disk.
///
/// THE ENCODING THE ECOSYSTEM ALREADY USES — the same shape a registry's own signing key is
/// configured with. A second encoding for the same kind of secret is a second thing to get wrong, and
/// two files holding one would be indistinguishable to whoever is looking at them.
pub const SEED_LEN: usize = 32;

/// Read a publisher signing key from a file.
pub fn load_signing_key(path: &Path) -> Result<[u8; SEED_LEN]> {
    // THE PATH IS NAMED WHEN IT IS ABSENT. A key that is not where the developer said it is is the
    // ordinary mistake, and a message about decoding would send them to look at its contents instead.
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("no publisher signing key at {}", path.display()))?;

    refuse_if_world_readable(path)?;

    let hex_text = text.trim();
    let bytes = hex::decode(hex_text)
        .with_context(|| format!("{} is not a hex-encoded signing key", path.display()))?;

    // THE LENGTH IS CHECKED BEFORE ANYTHING USES IT, and both lengths are named. A library that padded
    // or truncated a seed would produce a key that signs consistently and matches nothing — the
    // failure would surface as a signature that verifies nowhere, naming no file.
    if bytes.len() != SEED_LEN {
        bail!(
            "{} holds a {}-byte key; an Ed25519 signing seed is {SEED_LEN} bytes ({} hex characters)",
            path.display(),
            bytes.len(),
            SEED_LEN * 2
        );
    }
    let mut seed = [0u8; SEED_LEN];
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

/// Create a new publisher signing key, and return its PUBLIC half.
pub fn generate_signing_key(path: &Path) -> Result<[u8; 32]> {
    // REFUSED, AND NO FLAG OVERRIDES IT. Replacing a publisher key destroys the only copy of something
    // that cannot be reissued: every node that installed the package pins the old publisher, rotation
    // is refused, and the package becomes permanently un-updatable on all of them. There is no default
    // for which that is reasonable.
    if path.exists() {
        bail!(
            "{} already exists, and a publisher key is never replaced.\n  \
             Every node that installed a package signed with the existing key PINS that publisher: a \
             different key is refused, so overwriting this file would make the package permanently \
             un-updatable on every node that has it.",
            path.display()
        );
    }

    // FROM A CRYPTOGRAPHICALLY SECURE SOURCE, never derived from anything a developer chose. A key
    // derived from a passphrase is a key an attacker can search offline, and there is no rotation to
    // recover with.
    use rand::RngCore;
    let mut seed = [0u8; SEED_LEN];
    rand::rngs::OsRng.fill_bytes(&mut seed);

    let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
    let public = signing.verifying_key().to_bytes();

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    write_owner_only(path, hex::encode(seed).as_bytes())?;

    Ok(public)
}

/// What a new key commits its owner to, said at the moment of creation.
///
/// A CONSTANT RATHER THAN A `println!` AT THE CALL SITE, so the statement cannot drift away from the
/// operation that makes it true, and so a second caller cannot mint a key while saying less.
pub const COMMITMENT: &str = "\
This key is a one-way door for every package you sign with it:

  * every node that installs such a package PINS this publisher;
  * a bundle offering a DIFFERENT publisher key is refused — rotation is not built yet;
  * so losing this file means a package that can never be updated on any node that already has it.

Back it up somewhere you would trust with a production secret, and do not commit it.";

/// Write a secret with owner-only permissions.
///
/// THE PERMISSION IS SET BEFORE THE BYTES LAND AT THE FINAL PATH. A file created with default
/// permissions and tightened afterwards is world-readable for the length of that window, and a private
/// key is exactly the file for which the window matters.
#[cfg(unix)]
fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// On Windows the user profile directory is already ACL'd to its owner and there is no portable mode
/// bit. `create_new` still refuses an existing file, which is the half that matters most here.
#[cfg(not(unix))]
fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Refuse a key file anyone on the machine can read.
///
/// REFUSED RATHER THAN WARNED. A signing key whose permissions let anyone read it belongs to anyone on
/// the machine, and using it anyway would treat the developer's mistake as consent — while a warning
/// on a path that then succeeds is a warning nobody reads twice.
#[cfg(unix)]
fn refuse_if_world_readable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("reading the permissions of {}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        bail!(
            "{} is readable by other users (mode {:o}). A signing key anyone on this machine can read \
             belongs to anyone on this machine.\n  Run: chmod 600 {}",
            path.display(),
            mode & 0o777,
            path.display()
        );
    }
    Ok(())
}

/// Windows has no mode bits to inspect portably, and inventing an ACL check that answered "probably
/// fine" would be worse than saying nothing — a check that cannot fail is one a reader trusts.
#[cfg(not(unix))]
fn refuse_if_world_readable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
#[path = "publisher_key_adapter.test.rs"]
mod tests;
