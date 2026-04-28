/// Deterministic .ns generation for namespace tree directories.
///
/// UUIDs are generated from SHA1 of the full namespace path so they are stable
/// across builds — the same entity always gets the same UUID.
use anyhow::Result;
use sha1::{Digest, Sha1};
use std::path::Path;

/// Generate a deterministic UUID v5-ish from the entity's full namespace path.
/// Stable: same path → same UUID, always.
pub fn deterministic_uuid(namespace_path: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(namespace_path.as_bytes());
    let hash = hasher.finalize();

    let mut b = [0u8; 16];
    b.copy_from_slice(&hash[..16]);
    // Set version 5 and variant bits
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3],
        b[4], b[5],
        b[6], b[7],
        b[8], b[9],
        b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Infer the entity type for a directory in the namespace tree.
///
/// Rules:
/// - Depth 1 (e.g. `syw/`)                         → `organization`
/// - Depth 2 (e.g. `syw/network/`)                 → `folder` (unless it IS the package ns)
/// - Depth == package namespace depth AND matches   → `package`
/// - Deeper than package ns, contains type.json     → `type`
/// - Deeper than package ns, contains blueprint.json → `blueprint`
/// - Otherwise                                      → `folder`
pub fn infer_entity_type(dir: &Path, pkg_namespace: &str, namespace_root: &Path) -> String {
    let rel = dir.strip_prefix(namespace_root).unwrap_or(dir);
    let depth = rel.components().count();

    let ns_parts: Vec<&str> = pkg_namespace.split('.').collect();
    let ns_depth = ns_parts.len();

    if depth == 0 {
        return "folder".into();
    }

    if depth == 1 {
        return "organization".into();
    }

    // Check if this IS the package's own namespace directory
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let ns_as_path = ns_parts.join("/");
    if depth == ns_depth && rel_str == ns_as_path {
        return "package".into();
    }

    // Sub-entities within the package namespace
    if depth > ns_depth {
        if dir.join("blueprint.json").exists() {
            return "blueprint".into();
        }
        if dir.join("type.json").exists() {
            return "type".into();
        }
    }

    "folder".into()
}

/// Generate .ns for every directory under `namespace_root` that doesn't
/// already have one. Skips dev-authored .ns and legacy segment.json files.
pub fn generate_segment_manifests(
    namespace_root: &Path,
    pkg_namespace: &str,
    pkg_display_name: &str,
) -> Result<Vec<std::path::PathBuf>> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut generated = Vec::new();

    for entry in walkdir::WalkDir::new(namespace_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.path() != namespace_root)
    {
        let dir = entry.path();
        let ns_path = dir.join(".ns");
        let legacy_path = dir.join("segment.json");

        // Respect existing manifests
        if ns_path.exists() || legacy_path.exists() {
            continue;
        }

        let rel = dir.strip_prefix(namespace_root).unwrap_or(dir);
        let rel_dot = rel.to_string_lossy().replace(['\\', '/'], ".");
        let stable_id = deterministic_uuid(&format!("{}.{}", pkg_namespace, rel_dot));

        let entity_type = infer_entity_type(dir, pkg_namespace, namespace_root);
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let display_name = if entity_type == "package" {
            pkg_display_name.to_string()
        } else {
            name.clone()
        };

        // Format: id|name|display_name|type|created_at[|tags_csv]
        let content = format!(
            "{}|{}|{}|{}|{}",
            stable_id, name, display_name, entity_type, now
        );

        std::fs::write(&ns_path, content)?;
        generated.push(ns_path);
    }

    Ok(generated)
}
