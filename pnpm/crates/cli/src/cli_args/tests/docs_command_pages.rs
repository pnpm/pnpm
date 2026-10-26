//! Fails when the command reference in `pnpm/docs/cli` drifts from the
//! clap definitions: a page documents a flag the command rejects, or a
//! command accepts a flag its page does not document.
//!
//! A command's page is `cli/<name>.md`, or for a nested command the first
//! of `cli/<parent>-<name>.md` and `cli/<parent>.md` that exists. A page
//! documents a flag with a heading that starts with it, such as
//! `### --save-dev, -D`, and the headings of the `.mdx` partials it
//! imports count as its own. A page also documents the flags of every page
//! it delegates to with a sentence such as "accepts the options of
//! [`pnpm install`](./install.md)". A `--no-` flag and its positive form
//! document each other. Global flags are documented with a flag heading or
//! a heading naming their setting anywhere under `pnpm/docs`. A flag that
//! spells a setting is accepted by every command, since the config
//! overrides claim it before clap parses the command line.

use super::CliArgs;
use crate::config_overrides::ConfigOverrides;
use clap::{Arg, Command, CommandFactory};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fmt::Write as _,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

// Each list below holds today's gaps of one kind, one entry per gap. A gap
// missing from its list fails the test, and so does an entry that is no
// longer a gap, so fixing the docs means deleting the entry.

/// Flags a command accepts that its page does not document, as
/// `<command> <flag>`.
const UNDOCUMENTED_FLAGS: &[&str] = &[];

/// Flags a page documents that the command rejects, as `<page> <flag>`.
const DOCUMENTED_UNKNOWN_FLAGS: &[&str] = &[];

/// Commands with no page under `pnpm/docs/cli`.
const COMMANDS_WITHOUT_A_PAGE: &[&str] = &[
    // Not implemented: the command says to use npm.
    "edit", "restart", "stop", // Not implemented: the command says to use npm.
    "xmas",
];

/// Global flags with no heading anywhere under `pnpm/docs`.
const UNDOCUMENTED_GLOBAL_FLAGS: &[&str] = &[];

/// Commands documented on the page of another command.
const PAGE_NAMES: &[(&str, &str)] = &[
    ("approve", "permissions"),
    ("dlx", "pnx"),
    ("get", "config"),
    ("ll", "list"),
    ("purge", "clean"),
    ("rb", "rebuild"),
    ("set", "config"),
    ("stars", "star"),
    ("undeprecate", "deprecate"),
    ("unstar", "star"),
];

/// Spellings `shorthands.rs` rewrites before clap parses the command line,
/// so every command accepts them.
const SHORTHAND_FLAGS: &[&str] = &["--silent", "-s"];

#[derive(Debug, Default)]
struct Gaps {
    undocumented_flags: BTreeSet<String>,
    documented_unknown_flags: BTreeSet<String>,
    commands_without_a_page: BTreeSet<String>,
    undocumented_global_flags: BTreeSet<String>,
}

/// The spellings a command accepts, and the ones its page has to document.
#[derive(Debug, Default)]
struct CommandFlags {
    accepted: BTreeSet<String>,
    required: BTreeSet<String>,
}

/// The flags a page documents with its own headings, and with those plus
/// the headings of the pages it delegates to.
#[derive(Debug, Default)]
struct PageFlags {
    own: BTreeSet<String>,
    all: BTreeSet<String>,
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

/// The flag spellings (`--long`, `-s`) a heading documents, or none when
/// the heading is prose or a subcommand.
fn heading_flags(heading: &str) -> Vec<String> {
    let text = heading
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace(['`', '\\'], "");
    if !text.starts_with('-') {
        return Vec::new();
    }
    text.split(',')
        .filter_map(|part| part.split_whitespace().next())
        .map(|spelling| spelling.split_once('=').map_or(spelling, |(flag, _)| flag))
        .filter(|spelling| spelling.starts_with('-'))
        .flat_map(expand_negatable)
        .collect()
}

/// `--[no-]flag` documents both `--flag` and `--no-flag`.
fn expand_negatable(spelling: &str) -> Vec<String> {
    match spelling.strip_prefix("--[no-]") {
        Some(name) => vec![format!("--{name}"), format!("--no-{name}")],
        None => vec![spelling.to_owned()],
    }
}

/// The other spelling of a negatable flag: `--no-x` for `--x`, `--x` for
/// `--no-x`.
fn negated(flag: &str) -> Option<String> {
    flag.strip_prefix("--no-")
        .map(|name| format!("--{name}"))
        .or_else(|| {
            flag.strip_prefix("--")
                .map(|name| format!("--no-{name}"))
        })
}

fn documents(flags: &BTreeSet<String>, flag: &str) -> bool {
    flags.contains(flag) || negated(flag).is_some_and(|other| flags.contains(&other))
}

/// Every line of `page` outside fenced code blocks.
fn prose_lines(page: &str) -> impl Iterator<Item = &str> {
    let mut in_fence = false;
    page.lines()
        .filter(move |line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                return false;
            }
            !in_fence
        })
}

fn heading_text(line: &str) -> Option<&str> {
    let heading = line.trim_start_matches('#');
    (heading.len() < line.len())
        .then(|| heading.strip_prefix(' '))
        .flatten()
        .map(str::trim)
}

fn imported_partial(line: &str) -> Option<&str> {
    let (_, source) = line.strip_prefix("import ")?.split_once(" from ")?;
    let source = source
        .trim()
        .trim_end_matches(';')
        .trim_matches(['\'', '"']);
    source.ends_with(".mdx").then_some(source)
}

/// The headings of `path`, with the headings of every imported `.mdx`
/// partial spliced in where the import sits.
fn headings(path: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for line in prose_lines(&read_doc(path)) {
        if let Some(partial) = imported_partial(line) {
            let partial_path = path
                .parent()
                .expect("doc has a parent dir")
                .join(partial);
            found.extend(headings(&partial_path));
        } else if let Some(heading) = heading_text(line) {
            found.push(heading.to_owned());
        }
    }
    found
}

/// The pages `line` delegates to: links to `./<page>.md` on a line that
/// says the command accepts, takes, or applies the options or arguments
/// of another.
fn delegated_pages(line: &str) -> Vec<String> {
    let lower = line.to_ascii_lowercase();
    let names_flags = lower.contains("option") || lower.contains("argument");
    let delegates = lower.contains("accept") || lower.contains("takes") || lower.contains("apply");
    if !names_flags || !delegates {
        return Vec::new();
    }
    line.split("](./")
        .skip(1)
        .filter_map(|tail| tail.split_once(".md"))
        .map(|(page, _)| format!("{page}.md"))
        .collect()
}

fn own_flags(page: &str) -> BTreeSet<String> {
    headings(&docs_dir().join(page))
        .iter()
        .flat_map(|heading| heading_flags(heading))
        .collect()
}

/// The flags documented on the pages that `page` delegates to, resolved
/// against the directory of `page`.
fn delegated_flags(page: &str, visited: &mut BTreeSet<String>) -> BTreeSet<String> {
    let dir = page.rsplit_once('/').map_or("", |(dir, _)| dir);
    prose_lines(&read_doc(&docs_dir().join(page)))
        .flat_map(delegated_pages)
        .map(|delegated| if dir.is_empty() { delegated } else { format!("{dir}/{delegated}") })
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|delegated| page_flags(&delegated, visited).all)
        .collect()
}

/// The flags documented on `page` (relative to `pnpm/docs`), and on the
/// pages it delegates to.
fn page_flags(page: &str, visited: &mut BTreeSet<String>) -> PageFlags {
    let own = own_flags(page);
    let mut all = own.clone();
    if visited.insert(page.to_owned()) {
        all.extend(delegated_flags(page, visited));
    }
    PageFlags { own, all }
}

fn camel_case(kebab: &str) -> String {
    let mut capitalize = false;
    kebab
        .chars()
        .filter_map(|ch| {
            if ch == '-' {
                capitalize = true;
                return None;
            }
            let ch = if capitalize { ch.to_ascii_uppercase() } else { ch };
            capitalize = false;
            Some(ch)
        })
        .collect()
}

/// The settings a flag may mirror: `fooBar` for `--foo-bar`, and both
/// `noFooBar` and `fooBar` for `--no-foo-bar`.
fn setting_names(flag: &str) -> Vec<String> {
    let name = flag.trim_start_matches("--");
    name.strip_prefix("no-")
        .into_iter()
        .chain(std::iter::once(name))
        .map(camel_case)
        .collect()
}

/// Every flag spelling and setting name a heading under `pnpm/docs`
/// documents.
fn documented_everywhere() -> BTreeSet<String> {
    WalkDir::new(docs_dir())
        .into_iter()
        .map(|entry| entry.expect("walk pnpm/docs").into_path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "md")
        })
        .flat_map(|path| headings(&path))
        .flat_map(|heading| {
            let mut names = heading_flags(&heading);
            names.push(heading.trim_start_matches("[no-]").to_owned());
            names
        })
        .collect()
}

/// Whether some heading under `pnpm/docs` documents the global flag, or the
/// setting it mirrors.
fn global_flag_is_documented(flag: &str, documented: &BTreeSet<String>) -> bool {
    documents(documented, flag)
        || setting_names(flag)
            .iter()
            .any(|setting| documented.contains(setting))
}

/// Whether the config overrides claim `flag` on the command line of the
/// command at `path` before clap sees it, as they do for a flag that spells
/// a setting. A setting that takes a value is tried with one.
fn is_bare_setting_flag(path: &[&str], flag: &str) -> bool {
    let argv = |tail: &[&str]| {
        std::iter::once("pnpm")
            .chain(path.iter().copied())
            .chain(tail.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>()
    };
    [argv(&[flag]), argv(&[flag, "value"])]
        .into_iter()
        .any(|argv| !ConfigOverrides::extract(argv).1.contains(&OsString::from(flag)))
}

fn long_spellings(arg: &Arg) -> Vec<String> {
    arg.get_long()
        .into_iter()
        .chain(arg.get_all_aliases().unwrap_or_default())
        .map(|name| format!("--{name}"))
        .collect()
}

fn short_spellings(arg: &Arg) -> Vec<String> {
    arg.get_short()
        .into_iter()
        .chain(arg.get_all_short_aliases().unwrap_or_default())
        .map(|short| format!("-{short}"))
        .collect()
}

/// The spellings of the flags among `args`: every spelling is accepted, and
/// the long form of each visible flag that `is_documented_here` has to be
/// documented.
fn flags_of<'a>(
    args: impl Iterator<Item = &'a Arg>,
    is_documented_here: impl Fn(&Arg) -> bool,
) -> CommandFlags {
    let mut flags = CommandFlags::default();
    for arg in args {
        if arg.is_positional() || arg.get_id() == "help" || arg.get_id() == "version" {
            continue;
        }
        let longs = long_spellings(arg);
        flags.accepted.extend(longs.iter().cloned());
        flags.accepted.extend(short_spellings(arg));
        if !arg.is_hide_set() && is_documented_here(arg) {
            flags.required.extend(longs.into_iter().take(1));
        }
    }
    flags
}

/// A command's own flags. Its global flags are documented on the global
/// pages.
fn command_flags(command: &Command) -> CommandFlags {
    flags_of(command.get_arguments(), |arg| !arg.is_global_set())
}

/// The global flags of the root command, which every subcommand accepts.
fn global_flags(root: &Command) -> CommandFlags {
    flags_of(root.get_arguments().filter(|arg| arg.is_global_set()), |_| true)
}

/// The page of the command at `path`, relative to `pnpm/docs`: under
/// `cli/`, or a guide at the docs root named after the command.
fn page_for(path: &[&str]) -> Option<String> {
    let names: Vec<&str> = path
        .iter()
        .map(|name| {
            PAGE_NAMES
                .iter()
                .find(|(command, _)| command == name)
                .map_or(*name, |(_, page)| *page)
        })
        .collect();
    (1..=names.len())
        .rev()
        .map(|depth| format!("cli/{}.md", names[..depth].join("-")))
        .chain(std::iter::once(format!("{}.md", names.join("-"))))
        .find(|page| docs_dir().join(page).is_file())
}

/// Every visible command and nested subcommand with its path from the root.
fn visible_commands<'a>(
    command: &'a Command,
    path: &[&'a str],
    into: &mut Vec<(Vec<&'a str>, &'a Command)>,
) {
    for subcommand in command.get_subcommands() {
        if subcommand.is_hide_set() {
            continue;
        }
        let mut subpath = path.to_vec();
        subpath.push(subcommand.get_name());
        into.push((subpath.clone(), subcommand));
        visible_commands(subcommand, &subpath, into);
    }
}

/// The commands documented on each page, keyed by page, recording the
/// commands that have none.
fn commands_per_page<'a>(
    commands: &[(Vec<&'a str>, &'a Command)],
    gaps: &mut Gaps,
) -> BTreeMap<String, Vec<(Vec<&'a str>, &'a Command)>> {
    let mut pages: BTreeMap<String, Vec<(Vec<&str>, &Command)>> = BTreeMap::new();
    for (path, command) in commands {
        match page_for(path) {
            Some(page) => pages
                .entry(page)
                .or_default()
                .push((path.clone(), command)),
            None => {
                gaps.commands_without_a_page.insert(path.join(" "));
            }
        }
    }
    pages
}

/// The flags the commands on `page` have to document and do not.
fn undocumented_flags(page: &PageFlags, commands: &[(Vec<&str>, &Command)]) -> Vec<String> {
    commands
        .iter()
        .flat_map(|(path, command)| {
            command_flags(command).required
                .into_iter()
                .filter(|flag| !documents(&page.all, flag))
                .map(|flag| format!("{} {flag}", path.join(" ")))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The flags `page` documents that none of its commands accepts.
fn documented_unknown_flags(
    page: &PageFlags,
    commands: &[(Vec<&str>, &Command)],
    global: &CommandFlags,
) -> Vec<String> {
    let accepted: BTreeSet<String> = commands
        .iter()
        .flat_map(|(_, command)| command_flags(command).accepted)
        .collect();
    page.own
        .iter()
        .filter(|flag| !accepted.contains(*flag) && !global.accepted.contains(*flag))
        .filter(|flag| !SHORTHAND_FLAGS.contains(&flag.as_str()))
        .filter(|flag| !commands.iter().any(|(path, _)| is_bare_setting_flag(path, flag)))
        .cloned()
        .collect()
}

fn find_gaps() -> Gaps {
    let root = CliArgs::command();
    let global = global_flags(&root);
    let mut gaps = Gaps::default();
    let documented = documented_everywhere();
    gaps.undocumented_global_flags = global.required
        .iter()
        .filter(|flag| !global_flag_is_documented(flag, &documented))
        .cloned()
        .collect();

    let mut commands = Vec::new();
    visible_commands(&root, &[], &mut commands);
    for (page, commands) in commands_per_page(&commands, &mut gaps) {
        let flags = page_flags(&page, &mut BTreeSet::new());
        gaps.undocumented_flags.extend(undocumented_flags(&flags, &commands));
        gaps.documented_unknown_flags.extend(
            documented_unknown_flags(&flags, &commands, &global)
                .into_iter()
                .map(|flag| format!("{page} {flag}")),
        );
    }
    gaps
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
        writeln!(report, "{kind}: {new:#?}").unwrap();
    }
    if !stale.is_empty() {
        writeln!(report, "{kind}, no longer gaps, so remove them from the list: {stale:#?}")
            .unwrap();
    }
}

#[test]
fn a_heading_documents_the_flags_it_starts_with() {
    assert_eq!(heading_flags("--save-dev, -D"), ["--save-dev", "-D"]);
    assert_eq!(heading_flags(r"--registry &lt;url\>"), ["--registry"]);
    assert_eq!(heading_flags(r"--reporter=&lt;name\>"), ["--reporter"]);
    assert_eq!(heading_flags("`--filter <package_selector>`"), ["--filter"]);
    assert_eq!(heading_flags("--[no-]bail"), ["--bail", "--no-bail"]);
    assert_eq!(heading_flags("prune"), Vec::<String>::new());
    assert_eq!(heading_flags("set &lt;key> &lt;value>"), Vec::<String>::new());
}

#[test]
fn a_negatable_flag_and_its_positive_form_document_each_other() {
    let flags = BTreeSet::from(["--optional".to_owned()]);
    assert!(documents(&flags, "--no-optional"));
    assert!(documents(&flags, "--optional"));
    assert!(!documents(&flags, "--offline"));
}

#[test]
fn a_global_flag_is_documented_by_its_setting_name() {
    assert_eq!(setting_names("--store-dir"), ["storeDir"]);
    assert_eq!(setting_names("--no-proxy"), ["proxy", "noProxy"]);
    let documented = BTreeSet::from(["color".to_owned()]);
    assert!(global_flag_is_documented("--color", &documented));
    assert!(!global_flag_is_documented("--registry", &documented));
}

#[test]
fn a_flag_that_spells_a_setting_is_accepted_by_every_command() {
    assert!(is_bare_setting_flag(&["install"], "--shamefully-hoist"));
    assert!(is_bare_setting_flag(&["install"], "--no-lockfile"));
    assert!(is_bare_setting_flag(&["prune"], "--trust-lockfile"));
    assert!(is_bare_setting_flag(&["store", "prune"], "--virtual-store-dir"));
    assert!(!is_bare_setting_flag(&["install"], "--not-a-setting"));
}

#[test]
fn a_sentence_about_options_delegates_to_the_pages_it_links() {
    assert_eq!(
        delegated_pages("`pnpm ci` accepts the options of [`pnpm install`](./install.md), such as"),
        ["install.md"],
    );
    assert_eq!(
        delegated_pages(
            "The [recursive options](./recursive.md#options), such as `--sort`, apply."
        ),
        ["recursive.md"],
    );
    assert_eq!(
        delegated_pages(
            "[Read about this option in the run command docs](./run.md#--report-summary)"
        ),
        Vec::<String>::new(),
    );
}

#[test]
fn command_reference_matches_the_commands_pnpm_accepts() {
    let gaps = find_gaps();
    let mut report = String::new();
    report_kind(
        &mut report,
        "flags without a heading on the command's page (UNDOCUMENTED_FLAGS)",
        &gaps.undocumented_flags,
        UNDOCUMENTED_FLAGS,
    );
    report_kind(
        &mut report,
        "documented flags the command rejects (DOCUMENTED_UNKNOWN_FLAGS)",
        &gaps.documented_unknown_flags,
        DOCUMENTED_UNKNOWN_FLAGS,
    );
    report_kind(
        &mut report,
        "commands without a page (COMMANDS_WITHOUT_A_PAGE)",
        &gaps.commands_without_a_page,
        COMMANDS_WITHOUT_A_PAGE,
    );
    report_kind(
        &mut report,
        "global flags without a heading under pnpm/docs (UNDOCUMENTED_GLOBAL_FLAGS)",
        &gaps.undocumented_global_flags,
        UNDOCUMENTED_GLOBAL_FLAGS,
    );
    assert!(report.is_empty(), "the command reference in pnpm/docs drifted:\n{report}");
}
