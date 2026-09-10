//! `sdk_cli::session-store` — the persisted session.
//!
//! Spec: `sdk_cli::session-store` / `isession-store` / `session_store_impl`.
//!
//! WHY A FILE AND NOT A KEYCHAIN. A keychain would be better and is not portable across the three
//! platforms this tool ships on without three implementations, of which two would be exercised by
//! nobody. The file is written with owner-only permissions and never inside a project tree, which
//! closes the failure that actually happens — a credential committed to a repository.
//!
//! IT INTERPRETS NOTHING IT HOLDS. No scope, role or entitlement is read out of a token; what a
//! credential may do is the registry's answer. A store that pre-judged authorization would be a
//! second opinion about permission, and the one that disagrees silently is always the local one.

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::types::{PersistedSession, RegistryCredential};

/// Where the session document lives.
///
/// THE USER'S OWN CONFIG DIRECTORY, NEVER A PROJECT TREE. A credential file that can be committed
/// eventually is.
pub fn session_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("no user configuration directory on this platform, so there is nowhere to keep a session")?;
    Ok(dir.join("waffler").join("session.json"))
}

/// Read the persisted session.
///
/// AN ABSENT FILE IS A FIRST RUN, NOT AN ERROR. A malformed one IS an error, naming the path:
/// silently replacing an unreadable credential file with an empty session is how a developer's
/// stored logins vanish with no event that says so.
pub fn load() -> Result<PersistedSession> {
    let path = session_path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).with_context(|| {
            format!(
                "{} is not a readable session file. It is refused rather than replaced: \
                 overwriting it would discard every stored login with no event that says so. \
                 Delete it to start over.",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PersistedSession::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// The credential held for one normalised registry URL, or none.
///
/// THE KEY IS WHAT THE RESOLVER PRODUCED. There is no second normalisation here, and adding one
/// would let two spellings of a registry hold two credentials and use whichever the current
/// command's spelling happened to find.
pub fn credential_for(session: &PersistedSession, registry: &str) -> Option<RegistryCredential> {
    session.credentials.get(registry).cloned()
}

/// Persist a credential, replacing any entry already held for that registry.
pub fn put_credential(credential: &RegistryCredential) -> Result<()> {
    let mut session = load()?;
    // REPLACE, NEVER APPEND. One credential per registry is the invariant, and a map keyed by the
    // registry enforces it by shape rather than by a check that could be forgotten. The record
    // written is always complete — a credential is never stored with cleared fields, because a
    // blanked record and an expired one need different sentences and would become indistinguishable.
    session.credentials.insert(credential.registry.clone(), credential.clone());
    write(&session)
}

/// Delete the credential for one registry.
///
/// DELETED, NOT BLANKED. A record with cleared fields is indistinguishable from an expired one, and
/// the two need different sentences: "you are not signed in to this registry" versus "your session
/// expired".
pub fn remove_credential(registry: &str) -> Result<bool> {
    let mut session = load()?;
    let existed = session.credentials.remove(registry).is_some();
    write(&session)?;
    // Removing an entry that is not there is SUCCESS, not an error — signing out twice is not a
    // mistake. The boolean lets the caller say which happened without making one of them a failure.
    Ok(existed)
}

/// The persisted `use` default, or none.
pub fn default_registry(session: &PersistedSession) -> Option<String> {
    session.default_registry.clone()
}

/// Persist the `use` default.
///
/// IT DOES NOT CREATE OR TOUCH A CREDENTIAL. Which registry a command addresses and whether there is
/// an identity for it are separate facts, and conflating them is how a `use` to an unauthenticated
/// registry looks like a working session until the first publish.
pub fn set_default_registry(registry: &str) -> Result<()> {
    let mut session = load()?;
    session.default_registry = Some(registry.to_string());
    write(&session)
}

/// Write the whole document back, atomically and owner-only.
fn write(session: &PersistedSession) -> Result<()> {
    let path = session_path()?;
    let parent = path.parent().expect("the session path always has a parent directory");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;

    let text = serde_json::to_string_pretty(session).context("encoding the session")?;

    // ATOMIC: a temporary file in the SAME directory, then a rename. A process interrupted mid-write
    // otherwise leaves a truncated document, and the next run reports every stored login as gone.
    // Same directory because a rename across filesystems is a copy, which is not atomic.
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text.as_bytes())
        .with_context(|| format!("writing {}", temporary.display()))?;
    restrict_to_owner(&temporary)?;
    std::fs::rename(&temporary, &path)
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Make the file readable only by its owner.
///
/// SET ON THE TEMPORARY FILE BEFORE THE RENAME, so the document is never briefly world-readable at
/// its final path. A permission applied afterwards leaves a window, and the window is when a
/// refresh token is on disk with default permissions.
#[cfg(unix)]
fn restrict_to_owner(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting permissions on {}", path.display()))
}

/// On Windows the user profile directory is already ACL'd to the owner, and there is no portable
/// mode bit to set. Stated rather than silently skipped: a reader should not have to infer from an
/// absent `cfg` branch that nothing happens here.
#[cfg(not(unix))]
fn restrict_to_owner(_path: &std::path::Path) -> Result<()> {
    Ok(())
}
