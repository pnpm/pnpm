use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{error::VersioningError, intents::CHANGES_DIR, pending::PENDING_CHANGELOGS_DIR};

/// Directory, under the parked changelogs, holding the parked
/// publications.
const PUBLICATIONS_DIR: &str = "publications";

/// The name and note of a publication that `pnpm version -r` composed for a
/// registry that stores release notes, parked until `pnpm publish -r
/// --batch` sends it and collected with its members' parked sections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkedPublication {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The `package@version` of every release in the publication, keyed by
    /// manifest name like the ledger.
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

fn publications_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir
        .join(CHANGES_DIR)
        .join(PENDING_CHANGELOGS_DIR)
        .join(PUBLICATIONS_DIR)
}

/// A publication is keyed by its first member: a `package@version` is
/// released once, so no two plans share it.
fn publication_path(workspace_dir: &Path, publication: &ParkedPublication) -> Option<PathBuf> {
    let key = publication.members.first()?.replace('/', "!");
    Some(publications_dir(workspace_dir).join(format!("{key}.json")))
}

pub fn write_parked_publication(
    workspace_dir: &Path,
    publication: &ParkedPublication,
) -> Result<(), VersioningError> {
    let Some(path) = publication_path(workspace_dir, publication) else {
        return Ok(());
    };
    let dir = publications_dir(workspace_dir);
    fs::create_dir_all(&dir).map_err(|source| VersioningError::Write { path: dir, source })?;
    let json = serde_json::to_string_pretty(publication).expect("serialize parked publication");
    fs::write(&path, format!("{json}\n")).map_err(|source| VersioningError::Write { path, source })
}

/// Every parked publication. A file that does not parse is skipped, so a
/// hand-edited file cannot block a release.
pub fn read_parked_publications(
    workspace_dir: &Path,
) -> Result<Vec<ParkedPublication>, VersioningError> {
    let dir = publications_dir(workspace_dir);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(VersioningError::Read { path: dir, source }),
    };
    let mut publications = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| VersioningError::Read { path: dir.clone(), source })?
            .path();
        if path
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let content = fs::read_to_string(&path)
            .map_err(|source| VersioningError::Read { path: path.clone(), source })?;
        publications.extend(serde_json::from_str::<ParkedPublication>(&content).ok());
    }
    publications.sort_by(|left, right| left.members.cmp(&right.members));
    Ok(publications)
}

/// Removes a parked publication. A missing file is not an error.
pub fn remove_parked_publication(
    workspace_dir: &Path,
    publication: &ParkedPublication,
) -> Result<(), VersioningError> {
    let Some(path) = publication_path(workspace_dir, publication) else {
        return Ok(());
    };
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(VersioningError::Remove { path, source }),
    }
}
