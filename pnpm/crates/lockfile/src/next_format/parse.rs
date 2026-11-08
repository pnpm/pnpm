//! Parsing a next-format document with its large sections in parallel.

use super::{NextLockfile, NextProjectSnapshot, NextSnapshots};
use crate::{PackageKey, PackageMetadata};
use serde::Deserialize;
use std::collections::HashMap;

const SECTIONS: [&str; 3] = ["importers:", "packages:", "snapshots:"];

#[derive(Deserialize)]
struct Importers {
    #[serde(default)]
    importers: HashMap<String, NextProjectSnapshot>,
}

#[derive(Deserialize)]
struct Packages {
    packages: Option<HashMap<PackageKey, PackageMetadata>>,
}

#[derive(Deserialize)]
struct Snapshots {
    snapshots: Option<NextSnapshots>,
}

/// The document parsed as `importers`, `packages`, `snapshots`, and the rest,
/// on separate threads. `None` when the document does not split into those
/// blocks, or when any part fails to parse: the caller then parses it whole,
/// which reports an error at its place in the document.
pub(crate) fn parse_sections_in_parallel(
    document: &str,
    options: impl Fn(usize) -> serde_saphyr::Options + Sync,
) -> Option<NextLockfile> {
    let (rest, [importers, packages, snapshots]) = split_sections(document)?;
    let parse = |text: &str| -> Option<serde_saphyr::Options> {
        (!text.is_empty()).then(|| options(text.len()))
    };
    let ((rest, importers), (packages, snapshots)) = rayon::join(
        || {
            rayon::join(
                || {
                    serde_saphyr::from_str_with_options::<NextLockfile>(&rest, options(rest.len()))
                        .ok()
                },
                || match parse(importers) {
                    Some(options) => {
                        serde_saphyr::from_str_with_options::<Importers>(importers, options)
                            .ok()
                            .map(|section| section.importers)
                    }
                    None => Some(HashMap::new()),
                },
            )
        },
        || {
            rayon::join(
                || match parse(packages) {
                    Some(options) => {
                        serde_saphyr::from_str_with_options::<Packages>(packages, options)
                            .ok()
                            .map(|section| section.packages)
                    }
                    None => Some(None),
                },
                || match parse(snapshots) {
                    Some(options) => {
                        serde_saphyr::from_str_with_options::<Snapshots>(snapshots, options)
                            .ok()
                            .map(|section| section.snapshots)
                    }
                    None => Some(None),
                },
            )
        },
    );
    let mut lockfile = rest?;
    lockfile.importers = importers?;
    lockfile.packages = packages?;
    lockfile.snapshots = snapshots?;
    Some(lockfile)
}

/// The document's text outside the three sections, then the text of each
/// section, `""` for one that is absent. `None` unless each section is a
/// block that starts with its key alone on a line and appears once.
fn split_sections(document: &str) -> Option<(String, [&str; 3])> {
    let mut rest = String::with_capacity(1024);
    let mut sections: [Option<(usize, usize)>; 3] = [None; 3];
    let mut current: Option<(usize, usize)> = None;
    let mut offset = 0;
    for line in document.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let starts_top_level =
            line.starts_with(|first: char| !first.is_whitespace() && first != '#');
        if starts_top_level {
            if let Some((index, section_start)) = current.take() {
                sections[index] = Some((section_start, start));
            }
            if let Some(index) = SECTIONS
                .iter()
                .position(|key| line.trim_end() == *key)
            {
                if sections[index].is_some() {
                    return None;
                }
                current = Some((index, start));
                continue;
            }
            if SECTIONS
                .iter()
                .any(|key| line.starts_with(key))
            {
                return None;
            }
        }
        if current.is_none() {
            rest.push_str(line);
        }
    }
    if let Some((index, section_start)) = current {
        sections[index] = Some((section_start, document.len()));
    }
    Some((rest, sections.map(|range| range.map_or("", |(start, end)| &document[start..end]))))
}
