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
}

impl PublicationRef {
    /// The note's path from the server root, or `None` when the registry
    /// sent an `org` or `id` that is not a single path segment.
    #[must_use]
    pub fn notes_path(&self) -> Option<String> {
        (is_publication_segment(&self.org) && is_publication_segment(&self.id)).then(|| {
            format!("-/publications/{}/{}/notes", self.org, self.id)
        })
    }
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
