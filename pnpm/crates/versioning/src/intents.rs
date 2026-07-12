use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use derive_more::Display;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{error::VersioningError, human_id::random_human_id, settings::ReleaseBumpType};

/// The directory holding change-intent files, shared with changesets.
pub const CHANGES_DIR: &str = ".changeset";

/// A bump type as recorded in an intent file: a release bump or the additive
/// `none` decline ("this change needs no release").
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentBumpType {
    #[display("none")]
    None,
    #[display("patch")]
    Patch,
    #[display("minor")]
    Minor,
    #[display("major")]
    Major,
}

impl IntentBumpType {
    /// The release bump this entry demands; `None` for a decline.
    #[must_use]
    pub fn release(self) -> Option<ReleaseBumpType> {
        match self {
            IntentBumpType::None => None,
            IntentBumpType::Patch => Some(ReleaseBumpType::Patch),
            IntentBumpType::Minor => Some(ReleaseBumpType::Minor),
            IntentBumpType::Major => Some(ReleaseBumpType::Major),
        }
    }

    fn parse(value: &str) -> Option<IntentBumpType> {
        match value {
            "none" => Some(IntentBumpType::None),
            "patch" => Some(IntentBumpType::Patch),
            "minor" => Some(IntentBumpType::Minor),
            "major" => Some(IntentBumpType::Major),
            _ => None,
        }
    }
}

/// One `.changeset/*.md` file: which packages a change affects, the bump type
/// for each, and the summary that becomes the changelog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeIntent {
    pub id: String,
    pub file_path: PathBuf,
    pub releases: IndexMap<String, IntentBumpType>,
    pub summary: String,
}

pub fn parse_change_intent(
    content: &str,
    id: &str,
    file_path: &Path,
) -> Result<ChangeIntent, VersioningError> {
    let lines: Vec<&str> = content
        .trim_start_matches('\u{FEFF}')
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let closing_index = if lines.first().is_some_and(|line| line.trim() == "---") {
        lines.iter().skip(1).position(|line| line.trim() == "---").map(|index| index + 1)
    } else {
        None
    };
    let Some(closing_index) = closing_index else {
        return Err(VersioningError::NoFrontmatter { file_path: file_path.to_path_buf() });
    };

    let frontmatter_text = lines[1..closing_index].join("\n");
    let frontmatter: IndexMap<String, String> = if frontmatter_text.trim().is_empty() {
        IndexMap::new()
    } else {
        serde_saphyr::from_str(&frontmatter_text).map_err(|err| {
            VersioningError::InvalidFrontmatter {
                file_path: file_path.to_path_buf(),
                message: err.to_string(),
            }
        })?
    };

    let mut releases = IndexMap::new();
    for (pkg_name, bump_type) in frontmatter {
        let Some(parsed) = IntentBumpType::parse(&bump_type) else {
            return Err(VersioningError::InvalidBumpType {
                file_path: file_path.to_path_buf(),
                pkg_name,
                bump_type,
            });
        };
        releases.insert(pkg_name, parsed);
    }

    Ok(ChangeIntent {
        id: id.to_string(),
        file_path: file_path.to_path_buf(),
        releases,
        summary: lines[closing_index + 1..].join("\n").trim().to_string(),
    })
}

pub fn read_change_intents(workspace_dir: &Path) -> Result<Vec<ChangeIntent>, VersioningError> {
    let changes_dir = workspace_dir.join(CHANGES_DIR);
    let entries = match fs::read_dir(&changes_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(VersioningError::Read { path: changes_dir, source }),
    };

    let mut file_names = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|source| VersioningError::Read { path: changes_dir.clone(), source })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && !name.eq_ignore_ascii_case("readme.md") {
            file_names.push(name);
        }
    }
    file_names.sort();

    file_names
        .into_iter()
        .map(|file_name| {
            let file_path = changes_dir.join(&file_name);
            let content = fs::read_to_string(&file_path)
                .map_err(|source| VersioningError::Read { path: file_path.clone(), source })?;
            let id = file_name.trim_end_matches(".md");
            parse_change_intent(&content, id, &file_path)
        })
        .collect()
}

pub fn write_change_intent(
    workspace_dir: &Path,
    releases: &IndexMap<String, IntentBumpType>,
    summary: &str,
) -> Result<String, VersioningError> {
    let changes_dir = workspace_dir.join(CHANGES_DIR);
    fs::create_dir_all(&changes_dir)
        .map_err(|source| VersioningError::Write { path: changes_dir.clone(), source })?;

    let mut id = random_human_id();
    while changes_dir.join(format!("{id}.md")).exists() {
        id = random_human_id();
    }

    let frontmatter_lines: Vec<String> = releases
        .iter()
        .map(|(pkg_name, bump_type)| {
            format!("{}: {bump_type}", serde_json::to_string(pkg_name).expect("serialize string"))
        })
        .collect();
    let content = format!("---\n{}\n---\n\n{}\n", frontmatter_lines.join("\n"), summary.trim());
    let file_path = changes_dir.join(format!("{id}.md"));
    fs::write(&file_path, content)
        .map_err(|source| VersioningError::Write { path: file_path, source })?;
    Ok(id)
}

#[cfg(test)]
mod tests;
