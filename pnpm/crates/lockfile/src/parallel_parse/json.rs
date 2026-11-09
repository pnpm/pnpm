//! The JSON syntax's sections, parsed on separate threads. The next format
//! writes `projects` and `packages` with the header `  "<name>": {` alone
//! on a line, each entry starting at four spaces, and the closing `  },`
//! alone on a line, so the text splits at lines, without a parser. A
//! document laid out otherwise does not split, and is parsed whole.

use super::{
    CHUNKED_SECTION_BYTES, Importers, Packages, char_boundary_at_or_after, chunk_count, pool,
};
use crate::{LockfileDocument, json_document::from_json_str};
use rayon::prelude::*;
use serde::de::DeserializeOwned;
use std::collections::HashMap;

const SECTIONS: [&str; 2] = ["projects", "packages"];

/// The document parsed as `projects`, `packages` and the rest, on separate
/// threads. `None` when the document does not split into those sections,
/// or when any part fails to parse: the caller then parses it whole, which
/// reports an error at its place in the document.
pub(crate) fn parse_json_in_parallel<Importer, Snapshot, Package, Settings>(
    document: &str,
) -> Option<LockfileDocument<Importer, Snapshot, Package, Settings>>
where
    Importer: DeserializeOwned + Send,
    Snapshot: DeserializeOwned + Send,
    Package: DeserializeOwned + Send,
    Settings: DeserializeOwned + Send,
{
    let (rest, [importers, packages]) = split_sections(document)?;
    let ((rest, importers), packages) = pool()
        .join(
            || {
                rayon::join(
                    || {
                        from_json_str::<LockfileDocument<Importer, Snapshot, Package, Settings>>(
                            &rest,
                        )
                    },
                    || {
                        parse_chunked::<Importers<Importer>, _, _>(
                            "projects",
                            importers,
                            |section| section.importers,
                        )
                    },
                )
            },
            || {
                parse_chunked::<Packages<Package>, _, _>("packages", packages, |section| {
                    section.packages.unwrap_or_default()
                })
            },
        );
    let mut lockfile = rest.ok()?;
    if let Some(importers) = importers? {
        lockfile.importers = importers;
    }
    if let Some(packages) = packages? {
        lockfile.packages = Some(packages);
    }
    Some(lockfile)
}

/// The map of the section `name` whose entries are `body`, parsed in
/// parallel chunks when the section is large. `Some(None)` for an absent
/// section, `None` when a chunk fails to parse.
fn parse_chunked<Section, Key, Value>(
    name: &str,
    body: &str,
    map_of: impl Fn(Section) -> HashMap<Key, Value> + Sync,
) -> Option<Option<HashMap<Key, Value>>>
where
    Section: DeserializeOwned,
    Key: Eq + std::hash::Hash + Send,
    Value: Send,
{
    if body.is_empty() {
        return Some(None);
    }
    let count = if body.len() < CHUNKED_SECTION_BYTES { 1 } else { chunk_count(body.len()) };
    let maps = chunks(name, body, count)
        .into_par_iter()
        .map(|chunk| from_json_str::<Section>(&chunk).ok().map(&map_of))
        .collect::<Option<Vec<HashMap<Key, Value>>>>()?;
    let mut merged = HashMap::with_capacity(maps.iter().map(HashMap::len).sum());
    for map in maps {
        merged.extend(map);
    }
    Some(Some(merged))
}

/// `body` cut at entry boundaries into up to `count` documents of the form
/// `{"<name>": {<entries>}}`.
fn chunks(name: &str, body: &str, count: usize) -> Vec<String> {
    let target = body.len().div_ceil(count).max(1);
    let mut chunks = Vec::with_capacity(count);
    let mut start = 0;
    while start < body.len() {
        let end = next_entry_start(body, (start + target).min(body.len()));
        chunks.push(format!("{{\"{name}\": {{\n{}}}}}", &body[start..end]));
        start = end;
    }
    chunks
}

/// The start of the first entry at or after `from`: a line that starts with
/// four spaces and a quote. The end of `body` when there is none. `from` is
/// a byte offset that may fall inside a character.
pub(super) fn next_entry_start(body: &str, from: usize) -> usize {
    let from = char_boundary_at_or_after(body, from);
    if from >= body.len() {
        return body.len();
    }
    body[from..]
        .find("\n    \"")
        .map_or(body.len(), |offset| from + offset + 1)
}

/// The document with each section's entries removed, and the entries of
/// each section, `""` for an absent one. `None` unless every section found
/// has its header alone on a line, appears once, and closes with `  },` or
/// `  }` alone on a line.
fn split_sections(document: &str) -> Option<(String, [&str; 2])> {
    let mut splitter = Splitter::new();
    let mut offset = 0;
    for line in document.split_inclusive('\n') {
        splitter.take_line(line, offset)?;
        offset += line.len();
    }
    if splitter.current.is_some() {
        return None;
    }
    let sections =
        splitter.sections.map(|range| range.map_or("", |(start, end)| &document[start..end]));
    Some((splitter.rest, sections))
}

struct Splitter {
    headers: [String; 2],
    rest: String,
    sections: [Option<(usize, usize)>; 2],
    /// The section being read, and the offset its entries start at.
    current: Option<(usize, usize)>,
}

impl Splitter {
    fn new() -> Self {
        Splitter {
            headers: SECTIONS.map(|name| format!(r#"  "{name}": {{"#)),
            rest: String::with_capacity(1024),
            sections: [None; 2],
            current: None,
        }
    }

    /// Assigns `line`, which starts at `offset`, to the rest or to the
    /// section being read. `None` when a section appears twice.
    fn take_line(&mut self, line: &str, offset: usize) -> Option<()> {
        let content = line.trim_end();
        if let Some((index, start)) = self.current {
            if content == "  }," || content == "  }" {
                self.sections[index] = Some((start, offset));
                self.rest.push_str(line);
                self.current = None;
            }
            return Some(());
        }
        self.rest.push_str(line);
        let Some(index) = self.headers
            .iter()
            .position(|header| content == header)
        else {
            return Some(());
        };
        if self.sections[index].is_some() {
            return None;
        }
        self.current = Some((index, offset + line.len()));
        Some(())
    }
}
