use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::ErrorKind,
    path::Path,
};

use crate::{error::VersioningError, intents::CHANGES_DIR};

pub const LEDGER_FILENAME: &str = "ledger.yaml";

/// The committed, append-only record of consumed change intents: maps
/// `<package name>@<released version>` to the ids of the intent files
/// consumed by that release. Consumption is scoped per package — an intent
/// file is fully consumed only once every package it names has an entry —
/// which is what makes cherry-picked releases on maintenance branches and
/// merge-backs safe, and what lets one intent be half-consumed by a package
/// on a prerelease line.
pub type Ledger = BTreeMap<String, Vec<String>>;

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
    new_entries: &Ledger,
) -> Result<Ledger, VersioningError> {
    let mut ledger = read_ledger(workspace_dir)?;
    if new_entries.is_empty() {
        return Ok(ledger);
    }
    for (key, ids) in new_entries {
        let merged = ledger.entry(key.clone()).or_default();
        for id in ids {
            if !merged.contains(id) {
                merged.push(id.clone());
            }
        }
        merged.sort();
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
/// name is scoped, since a leading `@` is a YAML indicator), each with a
/// two-space-indented id list.
fn render_ledger(ledger: &Ledger) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    for (key, ids) in ledger {
        if key.starts_with('@') {
            writeln!(output, "\"{key}\":").expect("write to string");
        } else {
            writeln!(output, "{key}:").expect("write to string");
        }
        for id in ids {
            writeln!(output, "  - {id}").expect("write to string");
        }
    }
    output
}

/// Which intent ids the ledger records for one package.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackageConsumption {
    /// Intent ids recorded against any released version of the package.
    pub all_ids: HashSet<String>,
    /// Intent ids recorded only against prerelease versions of the package.
    pub prerelease_only_ids: HashSet<String>,
}

/// Indexes the ledger by package name in a single pass. Packages without
/// entries map to an empty consumption, so lookups never miss.
#[must_use]
pub fn build_consumption_index(ledger: &Ledger) -> HashMap<String, PackageConsumption> {
    let mut stable_ids_by_pkg: HashMap<String, HashSet<String>> = HashMap::new();
    let mut prerelease_ids_by_pkg: HashMap<String, HashSet<String>> = HashMap::new();
    for (key, ids) in ledger {
        let Some(at_index) = key.rfind('@').filter(|&index| index > 0) else {
            continue;
        };
        let pkg_name = &key[..at_index];
        let version = &key[at_index + 1..];
        // Build metadata (after "+") may itself contain hyphens and never
        // makes a version a prerelease.
        let is_prerelease = version.split('+').next().is_some_and(|core| core.contains('-'));
        let by_pkg =
            if is_prerelease { &mut prerelease_ids_by_pkg } else { &mut stable_ids_by_pkg };
        by_pkg.entry(pkg_name.to_string()).or_default().extend(ids.iter().cloned());
    }

    let names: HashSet<String> =
        stable_ids_by_pkg.keys().chain(prerelease_ids_by_pkg.keys()).cloned().collect();
    names
        .into_iter()
        .map(|name| {
            let stable = stable_ids_by_pkg.remove(&name).unwrap_or_default();
            let prerelease = prerelease_ids_by_pkg.remove(&name).unwrap_or_default();
            let consumption = PackageConsumption {
                prerelease_only_ids: prerelease.difference(&stable).cloned().collect(),
                all_ids: stable.into_iter().chain(prerelease).collect(),
            };
            (name, consumption)
        })
        .collect()
}
