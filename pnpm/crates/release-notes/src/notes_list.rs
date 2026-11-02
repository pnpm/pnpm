use node_semver::Version;
use serde::Deserialize;

/// The `<registry-base>/<name>/-/notes` document: the package's notes,
/// newest version first.
#[derive(Debug, Default, Deserialize)]
pub struct NotesList {
    #[serde(default)]
    pub notes: Vec<NoteEntry>,
    /// Older entries were left out. A reader whose range reaches past the
    /// oldest entry reads the rest one version at a time.
    #[serde(default)]
    pub truncated: bool,
}

/// One version of a [`NotesList`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEntry {
    pub version: String,
    /// The version note. Absent when only the publication has one, or the
    /// note was deleted.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub initial_digest: Option<String>,
    #[serde(default)]
    pub publication: Option<PublicationRef>,
}

/// The publication a version came out of, when it has a note the reader may
/// read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationRef {
    pub org: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub initial_digest: Option<String>,
    /// The publication note's URL, an absolute path on the server, as the
    /// `Link` header of the version route gives it. A client resolves it
    /// against the registry URL rather than deriving it from `org` and
    /// `id`, which would miss a pnpr served under a path prefix.
    #[serde(default)]
    pub url: Option<String>,
}

/// Whether `value` can be a publication `<org>`, identifier, or name: one
/// path segment of `[a-zA-Z0-9._+-]+`, never `.` or `..`.
#[must_use]
pub fn is_publication_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
}

/// The longest publication name a registry accepts, in bytes: the longest
/// npm package name.
pub const MAX_PUBLICATION_NAME_BYTES: usize = 214;

/// Whether `value` can name a publication: a publication segment of at most
/// [`MAX_PUBLICATION_NAME_BYTES`] that is not a decimal number, since names
/// share one namespace with the identifiers a registry assigns.
#[must_use]
pub fn is_publication_name(value: &str) -> bool {
    is_publication_segment(value)
        && value.len() <= MAX_PUBLICATION_NAME_BYTES
        && !value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether an upgrade from `installed` to `target` passes `version`:
/// `installed < version <= target`, with prereleases only when `target` is
/// one, so an upgrade between stable versions does not list the betas in
/// between.
#[must_use]
pub fn is_in_upgrade_range(version: &Version, installed: &Version, target: &Version) -> bool {
    version > installed
        && version <= target
        && (version.pre_release.is_empty() || !target.pre_release.is_empty())
}

#[cfg(test)]
mod tests;
