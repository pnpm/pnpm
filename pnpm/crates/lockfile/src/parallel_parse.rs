//! Parsing a lockfile document with its large sections on separate threads.
//!
//! `importers`, `packages`, and `snapshots` are independent YAML blocks, and
//! together most of a lockfile. Parsing them in parallel takes the time of
//! the largest instead of the sum.

use crate::{LockfileDocument, PackageKey, PackageMetadata};
use serde::{Deserialize, de::DeserializeOwned};
use std::collections::HashMap;

const SECTIONS: [&str; 3] = ["importers:", "packages:", "snapshots:"];

#[derive(Deserialize)]
#[serde(bound(deserialize = "Importer: Deserialize<'de>"))]
struct Importers<Importer> {
    #[serde(default = "HashMap::new")]
    importers: HashMap<String, Importer>,
}

#[derive(Deserialize)]
struct Packages {
    packages: Option<HashMap<PackageKey, PackageMetadata>>,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "Snapshot: Deserialize<'de>"))]
struct Snapshots<Snapshot> {
    snapshots: Option<HashMap<PackageKey, Snapshot>>,
}

/// The document parsed as `importers`, `packages`, `snapshots`, and the rest,
/// on separate threads. `None` when the document does not split into those
/// blocks, or when any part fails to parse: the caller then parses it whole,
/// which reports an error at its place in the document.
pub(crate) fn parse_sections_in_parallel<Importer, Snapshot>(
    document: &str,
    options: impl Fn(usize) -> serde_saphyr::Options + Sync,
) -> Option<LockfileDocument<Importer, Snapshot>>
where
    Importer: DeserializeOwned + Send,
    Snapshot: DeserializeOwned + Send,
{
    let (rest, [importers, packages, snapshots]) = split_sections(document)?;
    let ((rest, importers), (packages, snapshots)) = rayon::join(
        || {
            rayon::join(
                || parse_optional::<LockfileDocument<Importer, Snapshot>>(&rest, &options),
                || parse_optional::<Importers<Importer>>(importers, &options),
            )
        },
        || {
            rayon::join(
                || parse_optional::<Packages>(packages, &options),
                || parse_optional::<Snapshots<Snapshot>>(snapshots, &options),
            )
        },
    );
    let mut lockfile = rest??;
    lockfile.importers = importers?.map(|section| section.importers).unwrap_or_default();
    lockfile.packages = packages?.and_then(|section| section.packages);
    lockfile.snapshots = snapshots?.and_then(|section| section.snapshots);
    Some(lockfile)
}

/// `text` parsed, `Some(None)` when it is empty, and `None` when it fails to
/// parse.
fn parse_optional<Section: DeserializeOwned>(
    text: &str,
    options: &impl Fn(usize) -> serde_saphyr::Options,
) -> Option<Option<Section>> {
    if text.is_empty() {
        return Some(None);
    }
    serde_saphyr::from_str_with_options(text, options(text.len())).ok().map(Some)
}

/// The document's text outside the three sections, then the text of each
/// section, `""` for one that is absent. `None` unless each section is a
/// block that starts with its key alone on a line and appears once.
fn split_sections(document: &str) -> Option<(String, [&str; 3])> {
    let mut splitter =
        Splitter { rest: String::with_capacity(1024), sections: [None; 3], current: None };
    let mut offset = 0;
    for line in document.split_inclusive('\n') {
        splitter.take_line(line, offset)?;
        offset += line.len();
    }
    splitter.close_section(document.len());
    let sections =
        splitter.sections.map(|range| range.map_or("", |(start, end)| &document[start..end]));
    Some((splitter.rest, sections))
}

struct Splitter {
    rest: String,
    sections: [Option<(usize, usize)>; 3],
    /// The section being read, and the offset its key line starts at.
    current: Option<(usize, usize)>,
}

impl Splitter {
    /// Assigns `line`, which starts at `offset`, to a section or to the rest.
    /// `None` when the document cannot be split.
    fn take_line(&mut self, line: &str, offset: usize) -> Option<()> {
        let starts_top_level =
            line.starts_with(|first: char| !first.is_whitespace() && first != '#');
        if !starts_top_level {
            if self.current.is_none() {
                self.rest.push_str(line);
            }
            return Some(());
        }
        self.close_section(offset);
        let Some(index) = SECTIONS
            .iter()
            .position(|key| line.trim_end() == *key)
        else {
            if SECTIONS
                .iter()
                .any(|key| line.starts_with(key))
            {
                return None;
            }
            self.rest.push_str(line);
            return Some(());
        };
        if self.sections[index].is_some() {
            return None;
        }
        self.current = Some((index, offset));
        Some(())
    }

    fn close_section(&mut self, end: usize) {
        if let Some((index, start)) = self.current.take() {
            self.sections[index] = Some((start, end));
        }
    }
}
