//! Fails when the settings reference in `pnpm/docs` drifts from the keys
//! [`WorkspaceSettings`](crate::WorkspaceSettings) and
//! [`MachineSettings`](crate::MachineSettings) read from config files.
//!
//! The reference is the index in `settings.md` plus the sections of
//! `settings/*.md` and the `_*.mdx` partials they import. Index entries that
//! link to another page document a setting when the linked anchor exists.

use super::super::settings_field_keys;
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    path::{Path, PathBuf},
};

// Each list below holds today's gaps of one kind, one name per gap. A gap
// missing from its list fails the test, and so does an entry that is no
// longer a gap, so fixing the docs means deleting the entry.

/// Settings read from config files that no section or index entry documents.
const UNDOCUMENTED_SETTINGS: &[&str] = &[
    // Documented under a heading on another page, with no entry under
    // "Settings documented elsewhere" in the index.
    "_auth",              // npmrc.md
    "allowUnusedPatches", // cli/patch.md
    "bootConfig",         // config-includes.md
    "forceLegacyDeploy",  // cli/deploy.md
    "provenance",         // cli/publish.md
    // Documented in the prose of another page, with no heading.
    "catalog",                   // catalogs.md
    "catalogs",                  // catalogs.md
    "changedFilesIgnorePattern", // filtering.md
    "configDependencies",        // config-dependencies.md
    "gitChecks",                 // cli/publish.md
    "gitTagVersion",
    "registry",         // registries.md
    "tagVersionPrefix", // cli/version.md
    "testPattern",      // filtering.md
    // Aliases, mentioned only in the section of the setting they alias.
    "auditConfig",            // audit.ignore
    "auditLevel",             // audit.level
    "cleanupUnusedCatalogs",  // catalogPrune
    "maxSockets",             // maxsockets
    "noproxy",                // noProxy
    "proxy",                  // httpProxy
    "remoteSideEffectsCache", // sideEffectsCache.remote
    "updateConfig",           // update
    // Replaced by allowBuilds, listed only in its migration note in settings/build.md.
    "ignoredBuiltDependencies",
    "neverBuiltDependencies",
    "onlyBuiltDependencies",
    // Command-line options that a config file can also set, documented only
    // as flags of their commands.
    "aggregateOutput",
    "bail",
    "ignoreWorkspace",
    "mergeGitBranchLockfiles",
    "offline",
    "patchesDir",
    "pending",
    "preferOffline",
    "reporterHidePrefix",
    "reverse",
    "saveCatalogName",
    "savePeer",
    "skipManifestObfuscation",
    "sort",
    "stream",
    // Documented nowhere.
    "packageLock",
    "scope",
];

/// Sections and index entries naming a setting pnpm reads from no config file.
const SECTIONS_FOR_UNKNOWN_SETTINGS: &[&str] = &[
    "npmPath", // settings/cli.md, a pnpm v11 setting
];

/// Sections in `settings/*.md` that the index in `settings.md` does not link.
const SECTIONS_MISSING_FROM_INDEX: &[&str] = &[];

/// Index entries in `settings.md` whose link target has no such anchor.
const INDEX_ENTRIES_WITHOUT_SECTION: &[&str] = &[];

#[derive(Debug)]
struct Heading {
    text: String,
    slug: String,
}

#[derive(Debug)]
struct IndexLink {
    text: String,
    target: String,
}

fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
}

fn read_doc(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// Heading lines outside fenced code blocks, with the headings of every
/// imported `.mdx` partial spliced in where the import sits.
fn heading_lines(path: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    for line in read_doc(path).lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            collect_heading_line(path, line, &mut lines);
        }
    }
    lines
}

fn collect_heading_line(path: &Path, line: &str, lines: &mut Vec<String>) {
    if let Some(partial) = imported_partial(line) {
        let partial_path = path
            .parent()
            .expect("doc has a parent dir")
            .join(partial);
        lines.extend(heading_lines(&partial_path));
    } else if let Some(heading) = line.trim_start_matches('#').strip_prefix(' ')
        && line.starts_with('#')
    {
        lines.push(heading.trim().to_string());
    }
}

fn imported_partial(line: &str) -> Option<&str> {
    let (_, source) = line.strip_prefix("import ")?.split_once(" from ")?;
    let source = source
        .trim()
        .trim_end_matches(';')
        .trim_matches(['\'', '"']);
    source.ends_with(".mdx").then_some(source)
}

/// The anchors Docusaurus gives the headings of `path`: an explicit
/// `{#id}`, or the github-slugger slug with a `-<n>` suffix on repeats.
fn headings(path: &Path) -> Vec<Heading> {
    let mut seen: Vec<String> = Vec::new();
    heading_lines(path)
        .into_iter()
        .map(|line| {
            let (text, slug) = match line.split_once(" {#") {
                Some((text, id)) => (text.to_string(), id.trim_end_matches('}').to_string()),
                None => (line.clone(), slugify(&line)),
            };
            let repeats = seen
                .iter()
                .filter(|earlier| **earlier == slug)
                .count();
            seen.push(slug.clone());
            let slug = if repeats == 0 { slug } else { format!("{slug}-{repeats}") };
            Heading { text, slug }
        })
        .collect()
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter_map(|ch| match ch {
            ' ' => Some('-'),
            '-' | '_' => Some(ch),
            _ if ch.is_alphanumeric() => Some(ch),
            _ => None,
        })
        .collect()
}

/// The setting names a heading or index entry names: one, or several
/// separated by `,` or `/`. Prose and CLI flags name none.
fn setting_names(text: &str) -> Vec<String> {
    text.split([',', '/'])
        .map(|part| part.trim().trim_start_matches("[no-]"))
        .filter(|part| is_setting_name(part))
        .map(str::to_string)
        .collect()
}

fn is_setting_name(text: &str) -> bool {
    text.starts_with(|first: char| first.is_ascii_lowercase() || first == '_')
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_')
}

fn top_level_key(name: &str) -> &str {
    name.split_once('.')
        .map(|(key, _)| key)
        .unwrap_or(name)
}

/// The links in the list items of the `## Settings` section of `settings.md`.
fn index_links(settings_md: &str) -> Vec<IndexLink> {
    let mut lines = settings_md.lines();
    let has_index = lines.any(|line| line == "## Settings");
    assert!(has_index, "settings.md has no `## Settings` section");
    lines
        .filter_map(|line| line.trim_start().strip_prefix("* "))
        .flat_map(links_in)
        .collect()
}

fn links_in(mut line: &str) -> Vec<IndexLink> {
    let mut links = Vec::new();
    while let Some(start) = line.find('[') {
        line = &line[start..];
        let Some(end) = matching_bracket(line) else { break };
        let text = &line[1..end];
        line = &line[end + 1..];
        let Some((target, rest)) = line
            .strip_prefix('(')
            .and_then(|tail| tail.split_once(')'))
        else {
            continue;
        };
        links.push(IndexLink { text: text.to_string(), target: target.to_string() });
        line = rest;
    }
    links
}

/// The byte index of the `]` closing the `[` that `text` starts with.
fn matching_bracket(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in text.char_indices() {
        depth = match ch {
            '[' => depth + 1,
            ']' => depth - 1,
            _ => depth,
        };
        if depth == 0 {
            return Some(index);
        }
    }
    None
}

/// Whether `target`, a `./page.md` or `./page.md#anchor` link relative to the
/// docs root, points at an existing page and heading.
fn link_resolves(target: &str) -> bool {
    let (page, anchor) = target
        .split_once('#')
        .unwrap_or((target, ""));
    let path = page
        .trim_start_matches("./")
        .split('/')
        .fold(docs_dir(), |dir, segment| dir.join(segment));
    path.is_file()
        && (anchor.is_empty()
            || headings(&path)
                .iter()
                .any(|heading| heading.slug == anchor))
}

/// The setting names a resolving index link documents. A link without an
/// anchor points at a whole page, which documents no setting on its own.
fn names_documented_by(link: &IndexLink) -> Vec<String> {
    if link.target.contains('#') { setting_names(&link.text) } else { Vec::new() }
}

fn settings_pages() -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(docs_dir().join("settings"))
        .expect("read pnpm/docs/settings")
        .map(|entry| entry.expect("read pnpm/docs/settings entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "md")
        })
        .collect();
    pages.sort();
    pages
}

#[derive(Debug, Default)]
struct Gaps {
    undocumented_settings: BTreeSet<String>,
    sections_for_unknown_settings: BTreeSet<String>,
    sections_missing_from_index: BTreeSet<String>,
    index_entries_without_section: BTreeSet<String>,
}

fn find_gaps() -> Gaps {
    let code_keys: BTreeSet<&str> = settings_field_keys()
        .iter()
        .chain(crate::MachineSettings::keys())
        .map(String::as_str)
        .collect();
    let settings_md_path = docs_dir().join("settings.md");
    let links = index_links(&read_doc(&settings_md_path));
    let linked_targets: BTreeSet<String> = links
        .iter()
        .map(|link| link.target.trim_start_matches("./").to_string())
        .collect();

    let mut gaps = Gaps::default();
    let mut documented: BTreeSet<String> = headings(&settings_md_path)
        .iter()
        .flat_map(|heading| setting_names(&heading.text))
        .collect();

    for page in settings_pages() {
        documented.extend(page_setting_names(&page, &linked_targets, &mut gaps));
    }
    for link in &links {
        if link_resolves(&link.target) {
            documented.extend(names_documented_by(link));
        } else {
            gaps.index_entries_without_section.insert(link.text.clone());
        }
    }

    let documented_keys: BTreeSet<&str> = documented
        .iter()
        .map(|name| top_level_key(name))
        .collect();
    gaps.undocumented_settings = code_keys
        .iter()
        .filter(|key| !documented_keys.contains(*key))
        .map(|key| (*key).to_string())
        .collect();
    gaps.sections_for_unknown_settings = documented
        .iter()
        .filter(|name| !code_keys.contains(top_level_key(name)))
        .cloned()
        .collect();
    gaps
}

/// The setting names `page`'s sections document, recording each section the
/// index does not link.
fn page_setting_names(
    page: &Path,
    linked_targets: &BTreeSet<String>,
    gaps: &mut Gaps,
) -> Vec<String> {
    let file_name = page
        .file_name()
        .expect("page file name")
        .to_string_lossy()
        .into_owned();
    let mut documented = Vec::new();
    for heading in headings(page) {
        let names = setting_names(&heading.text);
        let target = format!("settings/{file_name}#{}", heading.slug);
        if !names.is_empty() && !linked_targets.contains(&target) {
            gaps.sections_missing_from_index.insert(heading.text);
        }
        documented.extend(names);
    }
    documented
}

fn report_kind(report: &mut String, kind: &str, found: &BTreeSet<String>, allowed: &[&str]) {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    let new: Vec<&String> = found
        .iter()
        .filter(|gap| !allowed.contains(gap.as_str()))
        .collect();
    let stale: Vec<&&str> = allowed
        .iter()
        .filter(|gap| !found.contains(**gap))
        .collect();
    if !new.is_empty() {
        writeln!(report, "{kind}: {new:?}").unwrap();
    }
    if !stale.is_empty() {
        writeln!(report, "{kind}, no longer gaps, so remove them from the list: {stale:?}").unwrap(
        );
    }
}

#[test]
fn an_index_link_to_a_whole_page_documents_no_setting() {
    let page_link = IndexLink { text: "catalog".to_string(), target: "./catalogs.md".to_string() };
    assert!(link_resolves(&page_link.target));
    assert_eq!(names_documented_by(&page_link), Vec::<String>::new());

    let section_link =
        IndexLink { text: "runtime".to_string(), target: "./settings/cli.md#runtime".to_string() };
    assert_eq!(names_documented_by(&section_link), ["runtime"]);
}

#[test]
fn settings_reference_matches_the_settings_pnpm_reads() {
    let gaps = find_gaps();
    let mut report = String::new();
    report_kind(
        &mut report,
        "settings without a section (UNDOCUMENTED_SETTINGS)",
        &gaps.undocumented_settings,
        UNDOCUMENTED_SETTINGS,
    );
    report_kind(
        &mut report,
        "sections for unknown settings (SECTIONS_FOR_UNKNOWN_SETTINGS)",
        &gaps.sections_for_unknown_settings,
        SECTIONS_FOR_UNKNOWN_SETTINGS,
    );
    report_kind(
        &mut report,
        "sections missing from the settings.md index (SECTIONS_MISSING_FROM_INDEX)",
        &gaps.sections_missing_from_index,
        SECTIONS_MISSING_FROM_INDEX,
    );
    report_kind(
        &mut report,
        "settings.md index entries without a section (INDEX_ENTRIES_WITHOUT_SECTION)",
        &gaps.index_entries_without_section,
        INDEX_ENTRIES_WITHOUT_SECTION,
    );
    assert!(report.is_empty(), "the settings reference in pnpm/docs drifted:\n{report}");
}
