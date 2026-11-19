use crate::{ComVer, Lockfile, LockfileVersion, ProjectSnapshot, extract_main_document};
use serde::Deserialize;
use std::{collections::HashMap, path::Path};

use super::{LoadLockfileError, read_lockfile_text, yaml_parse_options};

/// Whether a wanted-lockfile candidate contains a main document.
pub enum ImporterLoad {
    /// The file or its main document is absent.
    MissingLockfile,
    /// The main document is present, with the requested importer if found.
    Importer(Option<Box<ProjectSnapshot>>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImporterOnlyLockfile {
    lockfile_version: ComVer,
    #[serde(default, alias = "projects")]
    importers: HashMap<String, ProjectSnapshot>,
}

impl Lockfile {
    /// Load one project's importer without parsing the lockfile's package and
    /// snapshot maps. `None` when the file or importer is absent.
    pub fn load_importer_from_path(
        file_path: &Path,
        importer_id: &str,
    ) -> Result<ImporterLoad, LoadLockfileError> {
        let Some(content) = read_lockfile_text(file_path)? else {
            return Ok(ImporterLoad::MissingLockfile);
        };
        let main = extract_main_document(&content);
        if main.trim().is_empty() {
            return Ok(ImporterLoad::MissingLockfile);
        }
        let parsed: ImporterOnlyLockfile =
            serde_saphyr::from_str_with_options(&main, yaml_parse_options(main.len()))
                .map_err(|source| LoadLockfileError::parse_yaml(file_path, &source))?;
        if !LockfileVersion::<9>::is_compatible(parsed.lockfile_version) {
            return Ok(match Lockfile::load_from_path(file_path)? {
                Some(lockfile) => ImporterLoad::Importer(
                    lockfile.importers
                        .get(importer_id)
                        .cloned()
                        .map(Box::new),
                ),
                None => ImporterLoad::MissingLockfile,
            });
        }
        Ok(ImporterLoad::Importer(
            parsed.importers
                .get(importer_id)
                .cloned()
                .map(Box::new),
        ))
    }
}
