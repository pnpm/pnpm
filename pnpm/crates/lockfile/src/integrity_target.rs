use serde::{Deserialize, Serialize};

/// Which hash the `integrity` of a package tarball entry names: the
/// `lockfile.integrityTarget` setting, recorded as `settings.integrityTarget`.
///
/// Entries with a registry revision pin the tar under either value.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntegrityTarget {
    /// The resource at the tarball URL after HTTP content decoding: a
    /// `.tgz` on most registries, a tar on tar registries.
    #[default]
    Resource,
    /// The uncompressed tar, whatever compression the resource uses.
    Tar,
}
