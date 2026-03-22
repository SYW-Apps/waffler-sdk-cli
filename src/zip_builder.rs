/// ZIP archive construction for Waffler package format.
///
/// Output format:
///   package.json          ← manifest at ZIP root
///   <artifact>.wasm/.exe  ← compiled artifact at ZIP root (optional)
///   www/                  ← built UI assets (optional)
///   namespace/            ← namespace tree, merged into lib_dir on install
///     <ns_path>/
///       segment.json
///       <Entity>/
///         segment.json
///         type.json | blueprint.json
use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

pub struct ZipBuilder {
    entries: Vec<ZipEntry>,
}

struct ZipEntry {
    zip_path: String,
    source: PathBuf,
}

impl ZipBuilder {
    pub fn new() -> Self {
        Self { entries: vec![] }
    }

    /// Add a file at the specified path within the ZIP (relative to ZIP root).
    pub fn add_file(&mut self, source: &Path, zip_path: &str) -> &mut Self {
        self.entries.push(ZipEntry {
            zip_path: zip_path.to_string(),
            source: source.to_path_buf(),
        });
        self
    }

    /// Add all files under `dir` prefixed by `zip_prefix` in the ZIP.
    /// Skips files matching `exclude_names` (e.g. dev-only files).
    pub fn add_dir(&mut self, dir: &Path, zip_prefix: &str) -> anyhow::Result<&mut Self> {
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            let zip_path = if zip_prefix.is_empty() {
                rel.to_string()
            } else {
                format!("{}/{}", zip_prefix.trim_end_matches('/'), rel)
            };
            self.entries.push(ZipEntry {
                zip_path,
                source: entry.path().to_path_buf(),
            });
        }
        Ok(self)
    }

    /// Write all entries to a ZIP file at `output_path`.
    pub fn write(self, output_path: &Path) -> Result<()> {
        let file = std::fs::File::create(output_path)
            .with_context(|| format!("Cannot create ZIP file: {:?}", output_path))?;
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        for entry in &self.entries {
            zip.start_file(&entry.zip_path, options)
                .with_context(|| format!("ZIP entry error: {}", entry.zip_path))?;
            let data = std::fs::read(&entry.source)
                .with_context(|| format!("Cannot read {:?}", entry.source))?;
            zip.write_all(&data)
                .with_context(|| format!("Cannot write ZIP entry: {}", entry.zip_path))?;
        }

        zip.finish().context("Cannot finalize ZIP archive")?;
        Ok(())
    }
}
