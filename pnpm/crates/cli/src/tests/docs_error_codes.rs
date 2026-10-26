//! Fails when the error codes named in `pnpm/docs` drift from the
//! `ERR_PNPM_*` codes the source emits: a page names a code no source file
//! contains, or `errors.md` lists its codes out of order.
//!
//! The source is every Rust and JavaScript file under `pnpm/`, outside the
//! docs, tests, and `node_modules`. Codes only a test names are not emitted,
//! so they do not count. The migration guides name the codes of earlier
//! majors, so they are not checked.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Path, PathBuf},
};
use walkdir::{DirEntry, WalkDir};

// The list below holds today's gaps, one entry per gap. A gap missing from
// the list fails the test, and so does an entry that is no longer a gap, so
// fixing the docs means deleting the entry.

/// Codes a page names that no source file emits, as `<page> <code>`.
const UNKNOWN_CODES_IN_DOCS: &[&str] = &[];

const CODE_PREFIX: &str = "ERR_PNPM_";

fn pnpm_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// The `ERR_PNPM_*` codes in `text`. A backslash is dropped first, so a
/// Markdown-escaped `ERR\_PNPM\_X` reads as the code it names.
fn codes_in(text: &str) -> BTreeSet<String> {
    let text = text.replace('\\', "");
    text.match_indices(CODE_PREFIX)
        .map(|(start, _)| {
            let code = &text[start..];
            let end = code
                .find(|ch: char| !(ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_'))
                .unwrap_or(code.len());
            code[..end].trim_end_matches('_').to_owned()
        })
        .filter(|code| code.len() > CODE_PREFIX.len())
        .collect()
}

fn is_skipped_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && matches!(
            entry.file_name().to_str(),
            Some("docs" | "node_modules" | "target" | "test" | "tests")
        )
}

fn is_migration_guide(page: &str) -> bool {
    page.starts_with("migration")
}

fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "tests.rs" || name.ends_with(".test.js"))
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "rs" | "js" | "cjs" | "mjs"))
}

/// The codes the source under `dir` emits.
fn emitted_codes(dir: &Path) -> BTreeSet<String> {
    WalkDir::new(dir)
        .into_iter()
        .filter_entry(|entry| !is_skipped_dir(entry))
        .map(|entry| entry.expect("walk the source tree").into_path())
        .filter(|path| path.is_file() && is_source_file(path) && !is_test_file(path))
        .flat_map(|path| codes_in(&read(&path)))
        .collect()
}

/// The codes each page under `pnpm/docs` names, keyed by the page's path
/// relative to the docs directory.
fn documented_codes() -> BTreeMap<String, BTreeSet<String>> {
    let docs_dir = pnpm_dir().join("docs");
    WalkDir::new(&docs_dir)
        .into_iter()
        .map(|entry| entry.expect("walk pnpm/docs").into_path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "md" || extension == "mdx")
        })
        .map(|path| {
            let page = path
                .strip_prefix(&docs_dir)
                .expect("page is under pnpm/docs")
                .to_string_lossy()
                .replace('\\', "/");
            (page, codes_in(&read(&path)))
        })
        .collect()
}

/// The codes `errors.md` has a heading for, in page order.
fn error_page_headings() -> Vec<String> {
    read(&pnpm_dir().join("docs").join("errors.md"))
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .map(|heading| heading.replace('\\', ""))
        .collect()
}

#[test]
fn a_markdown_escaped_code_reads_as_the_code_it_names() {
    let codes =
        codes_in("fails with `ERR\\_PNPM\\_TARBALL\\_INTEGRITY` or ERR_PNPM_NO_OFFLINE_META.");
    assert_eq!(
        codes,
        BTreeSet::from([
            "ERR_PNPM_TARBALL_INTEGRITY".to_owned(),
            "ERR_PNPM_NO_OFFLINE_META".to_owned()
        ])
    );
    assert_eq!(codes_in("the ERR_PNPM_ prefix"), BTreeSet::new());
}

#[test]
fn every_code_the_docs_name_is_emitted() {
    let emitted = emitted_codes(&pnpm_dir());
    let found: BTreeSet<String> = documented_codes()
        .iter()
        .filter(|(page, _)| !is_migration_guide(page))
        .flat_map(|(page, codes)| {
            codes
                .iter()
                .filter(|code| !emitted.contains(*code))
                .map(move |code| format!("{page} {code}"))
        })
        .collect();
    let allowed: BTreeSet<&str> = UNKNOWN_CODES_IN_DOCS
        .iter()
        .copied()
        .collect();
    let new: Vec<&String> = found
        .iter()
        .filter(|gap| !allowed.contains(gap.as_str()))
        .collect();
    let stale: Vec<&&str> = allowed
        .iter()
        .filter(|gap| !found.contains(**gap))
        .collect();
    let mut report = String::new();
    if !new.is_empty() {
        writeln!(report, "codes no source emits (UNKNOWN_CODES_IN_DOCS): {new:#?}").unwrap();
    }
    if !stale.is_empty() {
        writeln!(report, "no longer gaps, so remove them from the list: {stale:#?}").unwrap();
    }
    assert!(report.is_empty(), "the error codes in pnpm/docs drifted:\n{report}");
}

#[test]
fn the_error_page_lists_its_codes_in_alphabetical_order() {
    let headings = error_page_headings();
    let unexpected: Vec<&String> = headings
        .iter()
        .filter(|heading| !heading.starts_with(CODE_PREFIX))
        .collect();
    assert!(unexpected.is_empty(), "errors.md has headings that are not codes: {unexpected:#?}");
    let mut sorted = headings.clone();
    sorted.sort();
    assert_eq!(headings, sorted, "errors.md must list its codes in alphabetical order");
}
