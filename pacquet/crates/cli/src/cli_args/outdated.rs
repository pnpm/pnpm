//! `pacquet outdated` — report direct dependencies that have a newer
//! version available.
//!
//! Ports pnpm's
//! [`outdated` command](https://github.com/pnpm/pnpm/blob/6f382f42ee/deps/inspection/commands/src/outdated/outdated.ts)
//! and the detection core in
//! [`@pnpm/deps.inspection.outdated`](https://github.com/pnpm/pnpm/blob/6f382f42ee/deps/inspection/outdated/src/outdated.ts).
//!
//! The detection half — [`collect_outdated`] — is shared with
//! `update --interactive`, which gathers the same "what has a newer
//! version" list before prompting. The two callers differ only in which
//! registry version counts as the comparison [`TargetVersion`]: `outdated`
//! compares against the absolute newest (`latest` tag, or the highest
//! in-range version under `--compatible`), while `update` compares against
//! the version a bump would move to.
//!
//! Scope vs. pnpm: pacquet loads a single lockfile (the *wanted*
//! lockfile), so there is no separate *current* lockfile to diff against —
//! a dependency's `current` and `wanted` versions are always equal, and
//! the "missing (wanted X)" state pnpm shows for a resolved-but-not-
//! installed dependency does not arise. Recursive (`-r`) and global
//! (`-g`) runs are rejected, matching `pacquet update`.

use crate::State;
use clap::{Args, ValueEnum};
use node_semver::Version;
use owo_colors::{OwoColorize, Stream};
use pacquet_config::{
    Config,
    matcher::{Matcher, create_matcher},
};
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::{Package, PackageVersion};
use std::{collections::HashMap, io::Write};

/// Which registry version a dependency is compared against to decide
/// whether it is outdated.
#[derive(Debug, Clone, Copy)]
pub enum TargetVersion {
    /// The `latest` dist-tag — the absolute newest published version.
    /// pnpm's default for `outdated`.
    Latest,
    /// The highest version satisfying the manifest range. pnpm's
    /// `outdated --compatible`, and the version an in-range `update`
    /// would move to.
    WithinRange,
}

/// A direct dependency with a newer (or deprecated) registry version.
///
/// `current` is the lockfile-pinned version; `target` is the resolved
/// [`TargetVersion`]. Both are always present — dependencies without a
/// lockfile pin, without a registry target, or whose specifier is not a
/// plain semver range are dropped during collection because they cannot
/// be diffed.
pub struct OutdatedPackage {
    /// The `package.json` key (and `node_modules` directory name). Equals
    /// `package_name` except for npm-alias entries (`"foo": "npm:bar@^1"`).
    pub alias: String,
    /// The registry package name actually queried.
    pub package_name: String,
    pub belongs_to: DependencyGroup,
    pub current: Version,
    pub target: Version,
    /// Deprecation reason of the `target` version, when the registry
    /// marked it deprecated.
    pub deprecated: Option<String>,
    /// `homepage` of the package, shown in the `--long` details column
    /// when the registry serves it.
    pub homepage: Option<String>,
}

/// What counts as outdated for a [`collect_outdated`] run.
pub struct OutdatedQuery<'a> {
    /// The registry version each dependency is compared against.
    pub target_version: TargetVersion,
    /// Dependency groups to inspect.
    pub include_direct: &'a [DependencyGroup],
    /// When present, restricts the walk to dependency keys the matcher
    /// accepts (pnpm's `outdated <pattern>` arguments).
    pub match_names: Option<&'a Matcher>,
    /// Also report a dependency whose `target` is deprecated even when it
    /// is not strictly newer than `current`. `outdated` sets this;
    /// `update` does not (a deprecated-but-current dependency has no
    /// newer version to move to).
    pub include_deprecated: bool,
}

/// Gather the direct dependencies whose `target` version is newer than
/// the lockfile-pinned `current` version (or, per
/// [`OutdatedQuery::include_deprecated`], whose `target` is deprecated).
///
/// A dependency the registry cannot serve (private, renamed, offline) is
/// skipped rather than failing the whole run.
pub async fn collect_outdated(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &ThrottledClient,
    query: &OutdatedQuery<'_>,
) -> miette::Result<Vec<OutdatedPackage>> {
    let current_versions = current_versions_from_lockfile(lockfile, query.include_direct);
    let current_versions = &current_versions;

    // Gather the lockfile-pinned direct dependencies to inspect, then
    // fetch their packuments concurrently — mirroring pnpm's
    // `Promise.all` fan-out. Concurrency is bounded by the HTTP client's
    // per-registry limit (`network_concurrency`), so this does not flood
    // the registry. Dependencies without a lockfile pin are dropped here;
    // those the registry cannot serve are dropped after the fetch.
    let fetches = query
        .include_direct
        .iter()
        .flat_map(move |&group| {
            manifest.dependencies([group]).filter_map(move |(alias, bare_specifier)| {
                if query.match_names.is_some_and(|matcher| !matcher.matches(alias)) {
                    return None;
                }
                let current = current_versions.get(alias).cloned()?;
                Some((alias, group, bare_specifier, current))
            })
        })
        .map(|(alias, group, bare_specifier, current)| async move {
            let (package_name, range) =
                PackageManifest::resolve_registry_dependency(alias, bare_specifier);
            let package = Package::fetch_from_registry(
                package_name,
                http_client,
                &config.registry,
                &config.auth_headers,
            )
            .await
            .ok()?;
            let target = resolve_target(&package, range, query.target_version)?;
            let deprecated = target.deprecated.clone();
            let is_newer = target.version > current;
            if !(is_newer || (query.include_deprecated && deprecated.is_some())) {
                return None;
            }
            Some(OutdatedPackage {
                alias: alias.to_string(),
                package_name: package_name.to_string(),
                belongs_to: group,
                current,
                target: target.version.clone(),
                deprecated,
                homepage: package.homepage,
            })
        });

    Ok(futures_util::future::join_all(fetches).await.into_iter().flatten().collect())
}

/// Resolve the [`TargetVersion`] to a concrete published version, or
/// `None` when the registry has no matching version (no `latest` tag, no
/// in-range version, or a non-semver range).
fn resolve_target(
    package: &Package,
    range: &str,
    target_version: TargetVersion,
) -> Option<std::sync::Arc<PackageVersion>> {
    match target_version {
        TargetVersion::Latest => {
            let tag = package.dist_tag("latest")?;
            package.versions.get(tag)
        }
        TargetVersion::WithinRange => {
            // `pinned_version` parses the range with `.unwrap()`, so guard
            // non-semver specifiers (`workspace:`, `link:`, git URLs) that
            // a lockfile-pinned semver `current` would not have screened
            // out on its own.
            range.parse::<node_semver::Range>().ok()?;
            package.pinned_version(range)
        }
    }
}

/// Map each direct dependency key to its lockfile-pinned semver version.
/// Only the root importer is consulted (pacquet's single-project scope);
/// entries whose resolved version is not a plain semver (`link:`,
/// `file:`, non-semver runtimes) are omitted.
fn current_versions_from_lockfile(
    lockfile: Option<&Lockfile>,
    include_direct: &[DependencyGroup],
) -> HashMap<String, Version> {
    let mut map = HashMap::new();
    let Some(importer) = lockfile.and_then(Lockfile::root_project) else { return map };
    for (name, spec) in importer.dependencies_by_groups(include_direct.iter().copied()) {
        if let Some(version) = spec.version.ver_peer().and_then(|ver| ver.version_semver()) {
            map.insert(name.to_string(), version.clone());
        }
    }
    map
}

/// Output format for `pacquet outdated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutdatedFormat {
    Table,
    List,
    Json,
}

/// `--prod` / `--dev` / `--no-optional` for `pacquet outdated`.
///
/// Ports pnpm's config normalization
/// ([`config/reader`](https://github.com/pnpm/pnpm/blob/6f382f42ee/config/reader/src/index.ts#L640-L650))
/// followed by the `include` map the `outdated` handler builds
/// ([`outdated.ts`](https://github.com/pnpm/pnpm/blob/6f382f42ee/deps/inspection/commands/src/outdated/outdated.ts#L182-L186)):
/// `--prod` keeps `dependencies` + `optionalDependencies`, `--dev` keeps
/// only `devDependencies`, and `--no-optional` drops
/// `optionalDependencies`. Note this differs from `update`'s include
/// formula.
#[derive(Debug, Args)]
pub struct OutdatedDependencyOptions {
    /// Check only "dependencies" and "optionalDependencies".
    #[clap(short = 'P', long, visible_alias = "production")]
    prod: bool,
    /// Check only "devDependencies".
    #[clap(short = 'D', long)]
    dev: bool,
    /// Don't check "optionalDependencies".
    #[clap(long)]
    no_optional: bool,
}

impl OutdatedDependencyOptions {
    fn include(&self) -> Vec<DependencyGroup> {
        let mut optional = !self.no_optional;
        let (production, dev) = if self.prod {
            (true, false)
        } else if self.dev {
            optional = false;
            (false, true)
        } else {
            (true, true)
        };
        std::iter::empty()
            .chain(production.then_some(DependencyGroup::Prod))
            .chain(dev.then_some(DependencyGroup::Dev))
            .chain(optional.then_some(DependencyGroup::Optional))
            .collect()
    }
}

/// `pacquet outdated [<pkg> ...]`.
#[derive(Debug, Args)]
pub struct OutdatedArgs {
    /// Restrict the check to dependencies whose name matches one of these
    /// patterns (`*` wildcard, leading `!` to negate). With no arguments,
    /// every direct dependency in the included groups is checked.
    pub packages: Vec<String>,

    /// --prod, --dev, and --no-optional.
    #[clap(flatten)]
    pub dependency_options: OutdatedDependencyOptions,

    /// Print only versions that satisfy the ranges in package.json.
    #[clap(long)]
    pub compatible: bool,

    /// Print details about the outdated packages (homepage, deprecation
    /// notice).
    #[clap(long)]
    pub long: bool,

    /// Output format.
    #[clap(long, value_enum, default_value_t = OutdatedFormat::Table)]
    pub format: OutdatedFormat,

    /// Shorthand for `--format list`. Good for small consoles.
    #[clap(long = "no-table")]
    pub no_table: bool,

    /// Shorthand for `--format json`.
    #[clap(long)]
    pub json: bool,

    /// Sorting method. Currently only `name` is supported; the default
    /// sorts by the size of the version change, then by name.
    #[clap(long, value_enum)]
    pub sort_by: Option<SortBy>,

    /// Check globally installed packages.
    #[clap(short = 'g', long)]
    pub global: bool,
}

/// `--sort-by` value. pnpm currently documents only `name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortBy {
    Name,
}

/// Whether `outdated` found any outdated dependency. The CLI harness maps
/// [`OutdatedOutcome::Outdated`] to a process exit code of `1`, matching
/// pnpm; returning the outcome (rather than terminating here) keeps
/// [`OutdatedArgs::run`] composable and process termination in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedOutcome {
    UpToDate,
    Outdated,
}

impl OutdatedArgs {
    /// Run the check and print the report to stdout. Returns whether any
    /// dependency was outdated; the caller decides the process exit code.
    pub async fn run(self, state: State) -> miette::Result<OutdatedOutcome> {
        if self.global {
            return Err(miette::miette!(
                "`pacquet outdated --global` is not supported yet; global package management has not been ported to pacquet."
            ));
        }
        if state.config.recursive {
            return Err(miette::miette!(
                "`pacquet outdated --recursive` is not supported yet; recursive workspace inspection has not been ported to pacquet."
            ));
        }

        let config = state.config;
        let manifest = &state.manifest;
        let lockfile = &state.lockfile;
        let http_client = &state.http_client;

        // A manifest with no dependencies at all is reported as up to date
        // (empty, exit 0) *before* the no-lockfile check — matching pnpm's
        // `packageHasNoDeps` short-circuit, so an empty project doesn't
        // error just because it was never installed.
        let has_any_dependency = manifest
            .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
            .next()
            .is_some();
        if !has_any_dependency {
            return Ok(OutdatedOutcome::UpToDate);
        }

        if lockfile.is_none() {
            let dir = manifest.path().parent().unwrap_or_else(|| manifest.path()).display();
            return Err(miette::miette!(
                code = "ERR_PNPM_OUTDATED_NO_LOCKFILE",
                "No lockfile in directory \"{dir}\". Run `pacquet install` to generate one."
            ));
        }

        let include = self.dependency_options.include();
        let target_version =
            if self.compatible { TargetVersion::WithinRange } else { TargetVersion::Latest };
        let matcher = (!self.packages.is_empty()).then(|| create_matcher(&self.packages));

        let query = OutdatedQuery {
            target_version,
            include_direct: &include,
            match_names: matcher.as_ref(),
            include_deprecated: true,
        };
        let mut outdated =
            collect_outdated(manifest, lockfile.as_ref(), config, http_client, &query).await?;

        sort_outdated(&mut outdated, self.sort_by);

        let output = match self.resolve_format() {
            OutdatedFormat::Table => render_table(&outdated, self.long),
            OutdatedFormat::List => render_list(&outdated, self.long),
            OutdatedFormat::Json => render_json(&outdated, self.long),
        };

        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{output}");
        let _ = stdout.flush();

        Ok(if outdated.is_empty() { OutdatedOutcome::UpToDate } else { OutdatedOutcome::Outdated })
    }

    /// Collapse the `--format` flag and its `--no-table` / `--json`
    /// shorthands into one format. The shorthands win over an explicit
    /// `--format`, with `--json` taking precedence over `--no-table`,
    /// mirroring pnpm's shorthand expansion order.
    fn resolve_format(&self) -> OutdatedFormat {
        if self.json {
            OutdatedFormat::Json
        } else if self.no_table {
            OutdatedFormat::List
        } else {
            self.format
        }
    }
}

/// The kind of semver bump from `current` to `target`. Drives the default
/// sort order and the colorized highlight in the `Latest` column.
///
/// Ports pnpm's [`@pnpm/semver-diff`](https://www.npmjs.com/package/@pnpm/semver-diff)
/// change classification: `None` for exactly-equal versions (pnpm's
/// `change: null`), `Fix` / `Feature` / `Breaking` for a patch / minor /
/// major difference, and `Unknown` when the versions differ only beyond
/// the major.minor.patch core (e.g. a prerelease-only bump), matching
/// pnpm's `change: 'unknown'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    None,
    Fix,
    Feature,
    Breaking,
    Unknown,
}

fn classify(current: &Version, target: &Version) -> Change {
    if current == target {
        Change::None
    } else if target.major != current.major {
        Change::Breaking
    } else if target.minor != current.minor {
        Change::Feature
    } else if target.patch != current.patch {
        Change::Fix
    } else {
        // Same major.minor.patch but not equal: a prerelease/build-only
        // difference. pnpm classifies this as `unknown`.
        Change::Unknown
    }
}

/// Ascending sort priority for the default (non-`--sort-by name`) order:
/// no-change, fix, feature, breaking, then unknown last. Mirrors pnpm's
/// `pkgPriority`.
fn change_priority(change: Change) -> u8 {
    match change {
        Change::None => 0,
        Change::Fix => 1,
        Change::Feature => 2,
        Change::Breaking => 3,
        Change::Unknown => 4,
    }
}

fn sort_outdated(outdated: &mut [OutdatedPackage], sort_by: Option<SortBy>) {
    match sort_by {
        Some(SortBy::Name) => {
            outdated.sort_by(|left, right| left.package_name.cmp(&right.package_name));
        }
        None => outdated.sort_by(|left, right| {
            let by_change = change_priority(classify(&left.current, &left.target))
                .cmp(&change_priority(classify(&right.current, &right.target)));
            by_change
                .then_with(|| left.package_name.cmp(&right.package_name))
                .then_with(|| left.current.to_string().cmp(&right.current.to_string()))
        }),
    }
}

fn render_table(outdated: &[OutdatedPackage], long: bool) -> String {
    if outdated.is_empty() {
        return String::new();
    }
    use tabled::builder::Builder;
    use tabled::settings::Style;

    let mut header: Vec<String> =
        ["Package", "Current", "Latest"].iter().map(|h| bright_blue(h)).collect();
    if long {
        header.push(bright_blue("Details"));
    }

    let mut builder = Builder::default();
    builder.push_record(header);
    for pkg in outdated {
        let mut row = vec![render_package_name(pkg), pkg.current.to_string(), render_latest(pkg)];
        if long {
            row.push(render_details(pkg));
        }
        builder.push_record(row);
    }
    let mut table = builder.build();
    table.with(Style::modern());
    table.to_string()
}

fn render_list(outdated: &[OutdatedPackage], long: bool) -> String {
    outdated
        .iter()
        .map(|pkg| {
            let mut info = format!(
                "{}\n{} {} {}",
                bold(&render_package_name(pkg)),
                pkg.current,
                grey("=>"),
                render_latest(pkg),
            );
            if long {
                let details = render_details(pkg);
                if !details.is_empty() {
                    info.push('\n');
                    info.push_str(&details);
                }
            }
            info
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_json(outdated: &[OutdatedPackage], long: bool) -> String {
    let mut map = serde_json::Map::new();
    for pkg in outdated {
        let dependency_type: &'static str = pkg.belongs_to.into();
        let mut entry = serde_json::json!({
            "current": pkg.current.to_string(),
            "latest": pkg.target.to_string(),
            "wanted": pkg.current.to_string(),
            "isDeprecated": pkg.deprecated.is_some(),
            "dependencyType": dependency_type,
        });
        if long {
            entry["latestManifest"] = serde_json::json!({
                "name": pkg.package_name,
                "version": pkg.target.to_string(),
                "deprecated": pkg.deprecated,
                "homepage": pkg.homepage,
            });
        }
        map.insert(pkg.package_name.clone(), entry);
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("serialize outdated report to JSON")
}

fn render_package_name(pkg: &OutdatedPackage) -> String {
    match pkg.belongs_to {
        DependencyGroup::Dev => format!("{} {}", pkg.package_name, dimmed("(dev)")),
        DependencyGroup::Optional => format!("{} {}", pkg.package_name, dimmed("(optional)")),
        _ => pkg.package_name.clone(),
    }
}

fn render_latest(pkg: &OutdatedPackage) -> String {
    let change = classify(&pkg.current, &pkg.target);
    if change == Change::None {
        return if pkg.deprecated.is_some() {
            red_bold("Deprecated")
        } else {
            pkg.target.to_string()
        };
    }
    let colored = colorize_version(&pkg.target, change);
    if pkg.deprecated.is_some() { format!("{colored} {}", red("(deprecated)")) } else { colored }
}

/// Highlight the version segment that changed: the whole string for a
/// breaking bump, from the minor field for a feature bump, from the patch
/// field for a fix. Mirrors pnpm's `colorizeSemverDiff`.
fn colorize_version(version: &Version, change: Change) -> String {
    let text = version.to_string();
    let split = match change {
        Change::Breaking => 0,
        Change::Feature => text.find('.').map_or(0, |i| i + 1),
        Change::Fix => {
            text.find('.').and_then(|i| text[i + 1..].find('.').map(|j| i + 1 + j + 1)).unwrap_or(0)
        }
        // pnpm's `colorizeSemverDiff` highlights nothing for an `unknown`
        // (or `null`) change, so the version renders plain.
        Change::None | Change::Unknown => return text,
    };
    let (head, tail) = text.split_at(split);
    let painted = match change {
        Change::Breaking => red(tail),
        Change::Feature => yellow(tail),
        Change::Fix => green(tail),
        Change::None | Change::Unknown => tail.to_string(),
    };
    format!("{head}{painted}")
}

fn render_details(pkg: &OutdatedPackage) -> String {
    let mut outputs = Vec::new();
    if let Some(reason) = &pkg.deprecated
        && !reason.is_empty()
    {
        outputs.push(red(reason));
    }
    if let Some(homepage) = &pkg.homepage {
        outputs.push(underline(homepage));
    }
    outputs.join("\n")
}

// Color helpers. Each is a no-op when stdout is not a terminal (piped or
// captured output), matching chalk's auto-disable so machine-readable
// output stays free of escape codes.
fn bright_blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bright_blue()).to_string()
}

fn red(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}

fn red_bold(text: &str) -> String {
    let style = owo_colors::Style::new().red().bold();
    text.if_supports_color(Stream::Stdout, |t| t.style(style)).to_string()
}

fn green(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.green()).to_string()
}

fn yellow(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.yellow()).to_string()
}

fn grey(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bright_black()).to_string()
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

fn dimmed(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

fn underline(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.underline()).to_string()
}

#[cfg(test)]
mod tests;
