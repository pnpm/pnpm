//! A project directory packed for publishing.

use pnpm_publish::PackedPkg;
use serde_json::Value;

pub(super) struct PackedDirectory {
    pub(super) project_dir: std::path::PathBuf,
    pub(super) source_manifest: Value,
    pub(super) published_manifest: Value,
    pub(super) tarball_data: Vec<u8>,
    pub(super) tarball_path: String,
    pub(super) contents: Vec<String>,
    pub(super) unpacked_size: u64,
    pub(super) notes: Option<String>,
}

impl PackedDirectory {
    pub(super) fn packed_pkg(&self) -> PackedPkg<'_> {
        PackedPkg {
            published_manifest: &self.published_manifest,
            tarball_data: &self.tarball_data,
            tarball_path: &self.tarball_path,
            contents: &self.contents,
            unpacked_size: self.unpacked_size,
            notes: self.notes.as_deref(),
        }
    }
}
