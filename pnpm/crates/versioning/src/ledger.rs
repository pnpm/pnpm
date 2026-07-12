use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::ErrorKind,
    path::Path,
};

use serde::Deserialize;

use crate::{error::VersioningError, intents::CHANGES_DIR};

pub const LEDGER_FILENAME: &str = "ledger.yaml";

/// One consumed release: the workspace-relative directory of the project
/// that released (the engine's unit of identity — package names may collide
/// across workspace projects) and the ids of the intent files the release
/// consumed. The bare id-list shape is accepted when read, for hand-written
/// entries; its project is then resolved by the package name in the entry
/// key, which must be unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum LedgerEntry {
    Ids(Vec<String>),
    Attributed { dir: String, intents: Vec<String> },
}

impl LedgerEntry {
    #[must_use]
    pub fn intent_ids(&self) -> &[String] {
        match self {
            LedgerEntry::Ids(ids) => ids,
            LedgerEntry::Attributed { intents, .. } => intents,
        }
    }
}

/// The committed, append-only record of consumed change intents: maps
/// `<package name>@<released version>` to the released project and the ids
/// of the intent files consumed by that release. Consumption is scoped per
/// project — an intent file is fully consumed only once every project it
/// names has an entry — which is what makes cherry-picked releases on
/// maintenance branches and merge-backs safe, and what lets one intent be
/// half-consumed by a package on a release lane.
pub type Ledger = BTreeMap<String, LedgerEntry>;

pub fn read_ledger(workspace_dir: &Path) -> Result<Ledger, VersioningError> {
    let ledger_path = workspace_dir.join(CHANGES_DIR).join(LEDGER_FILENAME);
    let content = match fs::read_to_string(&ledger_path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Ledger::new()),
        Err(source) => return Err(VersioningError::Read { path: ledger_path, source }),
    };
    if content.trim().is_empty() {
        return Ok(Ledger::new());
    }
    serde_saphyr::from_str(&content).map_err(|_| VersioningError::InvalidLedger { ledger_path })
}

pub fn append_to_ledger(
    workspace_dir: &Path,
    new_entries: &BTreeMap<String, (String, Vec<String>)>,
) -> Result<Ledger, VersioningError> {
    let mut ledger = read_ledger(workspace_dir)?;
    if new_entries.is_empty() {
        return Ok(ledger);
    }
    for (key, (dir, ids)) in new_entries {
        let mut merged: Vec<String> =
            ledger.get(key).map(|entry| entry.intent_ids().to_vec()).unwrap_or_default();
        for id in ids {
            if !merged.contains(id) {
                merged.push(id.clone());
            }
        }
        merged.sort();
        ledger.insert(key.clone(), LedgerEntry::Attributed { dir: dir.clone(), intents: merged });
    }

    let changes_dir = workspace_dir.join(CHANGES_DIR);
    fs::create_dir_all(&changes_dir)
        .map_err(|source| VersioningError::Write { path: changes_dir.clone(), source })?;
    let ledger_path = changes_dir.join(LEDGER_FILENAME);
    fs::write(&ledger_path, render_ledger(&ledger))
        .map_err(|source| VersioningError::Write { path: ledger_path, source })?;
    Ok(ledger)
}

/// Renders the ledger in the same YAML shape the TypeScript side's
/// `yaml.stringify` produces: sorted `package@version` keys (quoted when the
/// name is scoped, since a leading `@` is a YAML indicator), each with the
/// released project directory and a two-space-indented id list.
fn render_ledger(ledger: &Ledger) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    for (key, entry) in ledger {
        writeln!(output, "{}:", yaml_scalar(key)).expect("write to string");
        if let LedgerEntry::Attributed { dir, .. } = entry {
            writeln!(output, "  dir: {}", yaml_scalar(dir)).expect("write to string");
            writeln!(output, "  intents:").expect("write to string");
            for id in entry.intent_ids() {
                writeln!(output, "    - {}", yaml_scalar(id)).expect("write to string");
            }
        } else {
            for id in entry.intent_ids() {
                writeln!(output, "  - {}", yaml_scalar(id)).expect("write to string");
            }
        }
    }
    output
}

/// Renders a string as a YAML scalar the way the TypeScript side's
/// `yaml.stringify` does: plain when unambiguous, otherwise double-quoted
/// with escapes. Directory paths and `human-id` intent ids are always plain,
/// so the two stacks produce byte-identical ledgers for real content; the
/// quoting only guards odd hand-written values (a `#`, `: `, leading `@`, ...)
/// from round-tripping wrong.
fn yaml_scalar(value: &str) -> String {
    if !needs_quoting(value) {
        return value.to_string();
    }
    use std::fmt::Write as _;
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str(r"\\"),
            '"' => escaped.push_str(r#"\""#),
            '\n' => escaped.push_str(r"\n"),
            '\r' => escaped.push_str(r"\r"),
            '\t' => escaped.push_str(r"\t"),
            '\0' => escaped.push_str(r"\0"),
            // Every other control character must be escaped too, or the
            // written scalar would be invalid YAML that `read_ledger` can no
            // longer parse. `char::is_control` covers only U+0000–U+001F and
            // U+007F–U+009F, so a two-digit `\xNN` always fits.
            control if control.is_control() => {
                write!(escaped, r"\x{:02X}", control as u32).expect("write to string");
            }
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

fn needs_quoting(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    // Leading indicator characters, or a value YAML would read as something
    // other than a plain string.
    if value.starts_with([
        '!', '&', '*', '[', ']', '{', '}', ',', '#', '|', '>', '@', '`', '"', '\'', '%', '?', ':',
        '-', ' ',
    ]) {
        return true;
    }
    if value.ends_with(' ') {
        return true;
    }
    matches!(value, "true" | "false" | "null" | "yes" | "no" | "~")
        || value.chars().any(char::is_control)
        // A colon or hash that YAML would treat as a key/comment boundary.
        || value.contains(": ")
        || value.ends_with(':')
        || value.contains(" #")
}

/// Which intent ids the ledger records for one project.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackageConsumption {
    /// Intent ids recorded against any released version of the project.
    pub all_ids: HashSet<String>,
    /// Intent ids recorded only against prerelease versions of the project.
    pub prerelease_only_ids: HashSet<String>,
}

/// Indexes the ledger by workspace-relative project directory in a single
/// pass. Bare id-list entries carry no directory, so their project is
/// resolved from the entry key's package name via `resolve_name_dirs`; a
/// name matching several projects cannot be attributed and is an error —
/// write such entries in the `dir`/`intents` shape instead. Entries whose
/// name no longer exists in the workspace are inert. Projects without
/// entries map to an empty consumption, so lookups never miss.
pub fn build_consumption_index(
    ledger: &Ledger,
    resolve_name_dirs: impl Fn(&str) -> Vec<String>,
) -> Result<HashMap<String, PackageConsumption>, VersioningError> {
    let mut stable_ids_by_dir: HashMap<String, HashSet<String>> = HashMap::new();
    let mut prerelease_ids_by_dir: HashMap<String, HashSet<String>> = HashMap::new();
    for (key, entry) in ledger {
        let Some(at_index) = key.rfind('@').filter(|&index| index > 0) else {
            continue;
        };
        let version = &key[at_index + 1..];
        let dir = match entry {
            LedgerEntry::Attributed { dir, .. } => normalize_project_dir(dir),
            LedgerEntry::Ids(_) => {
                let pkg_name = &key[..at_index];
                let dirs = resolve_name_dirs(pkg_name);
                match dirs.len() {
                    0 => continue,
                    1 => dirs.into_iter().next().expect("one element"),
                    _ => {
                        return Err(VersioningError::AmbiguousLedgerEntry {
                            key: key.clone(),
                            pkg_name: pkg_name.to_string(),
                            dirs,
                        });
                    }
                }
            }
        };
        // Build metadata (after "+") may itself contain hyphens and never
        // makes a version a prerelease.
        let is_prerelease = version.split('+').next().is_some_and(|core| core.contains('-'));
        let by_dir =
            if is_prerelease { &mut prerelease_ids_by_dir } else { &mut stable_ids_by_dir };
        by_dir.entry(dir).or_default().extend(entry.intent_ids().iter().cloned());
    }

    let names: HashSet<String> =
        stable_ids_by_dir.keys().chain(prerelease_ids_by_dir.keys()).cloned().collect();
    Ok(names
        .into_iter()
        .map(|dir| {
            let stable = stable_ids_by_dir.remove(&dir).unwrap_or_default();
            let prerelease = prerelease_ids_by_dir.remove(&dir).unwrap_or_default();
            let consumption = PackageConsumption {
                prerelease_only_ids: prerelease.difference(&stable).cloned().collect(),
                all_ids: stable.into_iter().chain(prerelease).collect(),
            };
            (dir, consumption)
        })
        .collect())
}

/// The canonical spelling of a workspace-relative project directory:
/// forward slashes, no leading `./`, no trailing slash.
#[must_use]
pub fn normalize_project_dir(dir: &str) -> String {
    let mut normalized = dir.replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_string();
    }
    normalized.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests;
