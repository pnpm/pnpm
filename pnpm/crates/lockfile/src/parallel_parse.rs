//! Parsing a lockfile document with its large sections on separate threads.
//!
//! `importers`, `packages`, and `snapshots` are independent YAML blocks, and
//! together most of a lockfile. Parsing them in parallel takes the time of
//! the largest instead of the sum. The next format's [JSON](json) splits
//! the same way.

pub(crate) use json::parse_json_in_parallel;

use crate::{LockfileDocument, PackageKey};
use rayon::prelude::*;
use serde::{Deserialize, de::DeserializeOwned};
use std::collections::HashMap;

mod json;

const SECTIONS: [&str; 3] = ["importers:", "packages:", "snapshots:"];

#[derive(Deserialize)]
#[serde(bound(deserialize = "Importer: Deserialize<'de>"))]
struct Importers<Importer> {
    #[serde(default = "HashMap::new")]
    importers: HashMap<String, Importer>,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "Package: Deserialize<'de>"))]
struct Packages<Package> {
    packages: Option<HashMap<PackageKey, Package>>,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "Snapshot: Deserialize<'de>"))]
struct Snapshots<Snapshot> {
    snapshots: Option<HashMap<PackageKey, Snapshot>>,
}

/// The pool the lockfile's parallel work runs on. Parsing happens before
/// pnpm configures rayon's global pool, and using that pool first would fix
/// its size, so this crate keeps a pool of its own.
pub(crate) fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("pnpm-lockfile-{index}"))
            .build()
            .expect("build the lockfile thread pool")
    })
}

/// The document parsed as `importers`, `packages`, `snapshots`, and the rest,
/// on separate threads. `None` when the document does not split into those
/// blocks, or when any part fails to parse: the caller then parses it whole,
/// which reports an error at its place in the document.
pub(crate) fn parse_sections_in_parallel<Importer, Snapshot, Package, Settings>(
    document: &str,
    options: impl Fn(usize) -> serde_saphyr::Options + Sync,
) -> Option<LockfileDocument<Importer, Snapshot, Package, Settings>>
where
    Importer: DeserializeOwned + Send,
    Snapshot: DeserializeOwned + Send,
    Package: DeserializeOwned + Send,
    Settings: DeserializeOwned + Send,
{
    let (rest, [importers, packages, snapshots]) = split_sections(document)?;
    let ((rest, importers), (packages, snapshots)) =
        parse_parts::<Importer, Snapshot, Package, Settings>(
            &rest,
            [importers, packages, snapshots],
            &options,
        );
    // A section that was not found as a block, as in a flow-style document,
    // was parsed as part of the rest and stays as parsed.
    let mut lockfile = rest??;
    if let Some(importers) = importers? {
        lockfile.importers = importers;
    }
    if let Some(packages) = packages? {
        lockfile.packages = Some(packages);
    }
    if let Some(snapshots) = snapshots? {
        lockfile.snapshots = Some(snapshots);
    }
    Some(lockfile)
}

/// A section longer than this is parsed in chunks, one per thread.
const CHUNKED_SECTION_BYTES: usize = 128 * 1024;

/// A section's map, parsed in parallel chunks when the section is large.
/// `Some(None)` for an absent section, `None` when a chunk fails to parse.
///
/// Each chunk is the section's key line and a run of whole entries, so it
/// parses as the section itself; the maps are merged.
fn parse_chunked<Section, Key, Value>(
    text: &str,
    options: &(impl Fn(usize) -> serde_saphyr::Options + Sync),
    map_of: impl Fn(Section) -> HashMap<Key, Value> + Sync,
) -> Option<Option<HashMap<Key, Value>>>
where
    Section: DeserializeOwned,
    Key: Eq + std::hash::Hash + Send,
    Value: Send,
{
    if text.is_empty() {
        return Some(None);
    }
    if text.len() < CHUNKED_SECTION_BYTES {
        return parse_optional::<Section>(text, options).map(|section| section.map(&map_of));
    }
    let chunks = chunk_entries(text, chunk_count(text.len()));
    let maps = chunks
        .into_par_iter()
        .map(|chunk| parse_optional::<Section>(&chunk, options).flatten().map(&map_of))
        .collect::<Option<Vec<HashMap<Key, Value>>>>()?;
    let mut merged = HashMap::with_capacity(maps.iter().map(HashMap::len).sum());
    for map in maps {
        merged.extend(map);
    }
    Some(Some(merged))
}

/// The start of the first top-level entry at or after `from`: a line that
/// starts with two spaces and a non-space. The end of `body` when there is
/// none. `from` is a byte offset that may fall inside a character.
pub(crate) fn next_entry_start(body: &str, from: usize) -> usize {
    let from = char_boundary_at_or_after(body, from);
    if from >= body.len() {
        return body.len();
    }
    body[from..]
        .match_indices("\n  ")
        .map(|(offset, _)| from + offset + 1)
        .find(|&at| body[at + 2..].starts_with(|first: char| !first.is_whitespace()))
        .unwrap_or(body.len())
}

/// The first character boundary at or after the byte offset `at`, so a cut
/// made by byte count never slices a character.
pub(crate) fn char_boundary_at_or_after(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// Enough chunks to spread a section over the threads, each at least
/// [`CHUNK_TARGET_BYTES`] so that the merge of the maps stays cheap.
fn chunk_count(len: usize) -> usize {
    let by_size = len.div_ceil(CHUNK_TARGET_BYTES);
    by_size.clamp(2, pool().current_num_threads() * 4)
}

const CHUNK_TARGET_BYTES: usize = 32 * 1024;

/// `text`, a section starting with its key line, cut into up to `count`
/// chunks at entry boundaries, each starting with the key line.
fn chunk_entries(text: &str, count: usize) -> Vec<String> {
    let header_end = text
        .find('\n')
        .map_or(text.len(), |end| end + 1);
    let (header, body) = text.split_at(header_end);
    let target = body.len().div_ceil(count).max(1);
    let mut chunks = Vec::with_capacity(count);
    let mut start = 0;
    while start < body.len() {
        let end = next_entry_start(body, (start + target).min(body.len()));
        chunks.push(format!("{header}{}", &body[start..end]));
        start = end;
    }
    chunks
}

type Parsed<Importer, Snapshot, Package, Settings> = (
    (
        Option<Option<LockfileDocument<Importer, Snapshot, Package, Settings>>>,
        Option<Option<HashMap<String, Importer>>>,
    ),
    (Option<Option<HashMap<PackageKey, Package>>>, Option<Option<HashMap<PackageKey, Snapshot>>>),
);

/// The rest and the three sections, parsed on the pool at once.
fn parse_parts<Importer, Snapshot, Package, Settings>(
    rest: &str,
    [importers, packages, snapshots]: [&str; 3],
    options: &(impl Fn(usize) -> serde_saphyr::Options + Sync),
) -> Parsed<Importer, Snapshot, Package, Settings>
where
    Importer: DeserializeOwned + Send,
    Snapshot: DeserializeOwned + Send,
    Package: DeserializeOwned + Send,
    Settings: DeserializeOwned + Send,
{
    pool()
        .join(
            || {
                rayon::join(
                    || {
                        parse_optional::<LockfileDocument<Importer, Snapshot, Package, Settings>>(
                            rest, options,
                        )
                    },
                    || {
                        parse_chunked::<Importers<Importer>, _, _>(importers, options, |section| {
                            section.importers
                        })
                    },
                )
            },
            || {
                rayon::join(
                    || {
                        parse_chunked::<Packages<Package>, _, _>(packages, options, |section| {
                            section.packages.unwrap_or_default()
                        })
                    },
                    || {
                        parse_chunked::<Snapshots<Snapshot>, _, _>(snapshots, options, |section| {
                            section.snapshots.unwrap_or_default()
                        })
                    },
                )
            },
        )
}

/// `text` parsed, `Some(None)` when it is empty, and `None` when it fails to
/// parse.
#[cfg(test)]
mod tests;

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
