//! Contract test: every pacquet default that maps to a pnpm CLI
//! setting must equal pnpm's own default.
//!
//! pnpm is the source of truth (see
//! [`pacquet/AGENTS.md`](../../AGENTS.md#the-cardinal-rule)). Its CLI
//! defaults live in one object literal, `defaultOptions`, in
//! [`config/reader/src/index.ts`](https://github.com/pnpm/pnpm/blob/a23956e3ab/config/reader/src/index.ts).
//! This test reads that literal from source at test time and compares
//! each value against [`Config::default()`], so a drift on either side
//! (pnpm changing a default, or pacquet hard-coding the wrong one — the
//! `dedupeDirectDeps` regression that broke the v11.5.1 release) fails
//! here instead of in a release pipeline.
//!
//! Three buckets classify every key in `defaultOptions`:
//!
//! - **mapped** — pacquet implements the setting with a directly
//!   comparable literal default; the value is asserted equal.
//! - **non-literal** — pacquet implements it, but pnpm's default is an
//!   environment / platform / CPU expression (`registry`,
//!   `workspace-concurrency`, ...) with no source literal to compare.
//! - **not ported** — pacquet has no `Config` field for it yet.
//!
//! The completeness guard asserts the three buckets exactly partition
//! pnpm's key set, so a *new* pnpm setting (neither mapped nor skipped)
//! fails the test until someone classifies it — which is how this test
//! keeps catching the next default that needs porting.

use crate::{
    CatalogMode, Config, LinkWorkspacePackages, NodeLinker, ResolutionMode, ScriptsPrependNodePath,
};
use std::collections::BTreeSet;

/// A pnpm default value reduced to the shapes this test compares.
#[derive(Debug, PartialEq, Eq)]
enum Scalar {
    Bool(bool),
    Int(i64),
    Str(String),
    /// Order-insensitive list of strings (pnpm's array defaults carry
    /// no meaningful order).
    Set(BTreeSet<String>),
    /// pnpm's `undefined` default — pacquet models it as a `None`
    /// `Option` field (e.g. `save-catalog-name`).
    Undefined,
}

fn s(value: &str) -> Scalar {
    Scalar::Str(value.to_string())
}

/// pnpm keys whose default is an environment / platform / CPU
/// expression, not a source literal. pacquet implements each, but there
/// is nothing to string-compare against, so they are exercised by the
/// dedicated `current::<Host>()` config-loading tests instead.
const NON_LITERAL: &[&str] = &[
    "registry",                     // npmDefaults.registry
    "unsafe-perm",                  // npmDefaults['unsafe-perm']
    "userconfig",                   // npmDefaults.userconfig (home-derived path)
    "virtual-store-dir-max-length", // isWindows() ? 60 : 120
    "workspace-concurrency",        // derived from CPU count
];

/// pnpm settings pacquet has no `Config` field for yet. Porting one
/// means moving its key from here into a `mapped` row.
const NOT_PORTED: &[&str] = &[
    "bail",
    "ci",
    "color",
    "deploy-all-files",
    "disallow-workspace-cycles",
    "embed-readme",
    "enable-modules-dir",
    "extend-node-path",
    "fail-if-no-match",
    "fetch-min-speed-ki-bps",
    "fetch-warn-timeout-ms",
    "force-legacy-deploy",
    "git-branch-lockfile",
    "ignore-workspace-cycles",
    "ignore-workspace-root-check",
    "init-package-manager",
    "init-type",
    "optional",
    "package-lock",
    "pending",
    "recursive-install",
    "reverse",
    "save-peer",
    "save-workspace-protocol",
    "shared-workspace-lockfile",
    "shell-emulator",
    "skip-manifest-obfuscation",
    "sort",
    "strict-dep-builds",
    "strict-store-pkg-content-check",
    "use-beta-cli",
    "verify-deps-before-run",
    "virtual-store-only",
    "workspace-prefix",
];

/// `(pnpm key, pacquet default rendered as a [`Scalar`])` for every
/// setting pacquet implements with a comparable literal default. The
/// test asserts each pacquet value equals the value pnpm's source
/// records under the same key.
fn mapped_rows(cfg: &Config) -> Vec<(&'static str, Scalar)> {
    use Scalar::{Bool, Int};
    vec![
        ("auto-install-peers", Bool(cfg.auto_install_peers)),
        ("block-exotic-subdeps", Bool(cfg.block_exotic_subdeps)),
        ("dangerously-allow-all-builds", Bool(cfg.dangerously_allow_all_builds)),
        ("dedupe-direct-deps", Bool(cfg.dedupe_direct_deps)),
        ("dedupe-injected-deps", Bool(cfg.dedupe_injected_deps)),
        ("dedupe-peer-dependents", Bool(cfg.dedupe_peer_dependents)),
        ("dedupe-peers", Bool(cfg.dedupe_peers)),
        ("enable-pre-post-scripts", Bool(cfg.enable_pre_post_scripts)),
        ("exclude-links-from-lockfile", Bool(cfg.exclude_links_from_lockfile)),
        ("hoist", Bool(cfg.hoist)),
        ("hoist-workspace-packages", Bool(cfg.hoist_workspace_packages)),
        ("inject-workspace-packages", Bool(cfg.inject_workspace_packages)),
        ("lockfile-include-tarball-url", Bool(cfg.lockfile_include_tarball_url)),
        (
            "minimum-release-age-ignore-missing-time",
            Bool(cfg.minimum_release_age_ignore_missing_time),
        ),
        ("optimistic-repeat-install", Bool(cfg.optimistic_repeat_install)),
        ("prefer-workspace-packages", Bool(cfg.prefer_workspace_packages)),
        ("registry-supports-time-field", Bool(cfg.registry_supports_time_field)),
        ("resolve-peers-from-workspace-root", Bool(cfg.resolve_peers_from_workspace_root)),
        ("side-effects-cache", Bool(cfg.side_effects_cache)),
        ("strict-peer-dependencies", Bool(cfg.strict_peer_dependencies)),
        ("symlink", Bool(cfg.symlink)),
        ("verify-store-integrity", Bool(cfg.verify_store_integrity)),
        // `boolean | 'deep'` upstream; the default is `false`.
        ("link-workspace-packages", link_workspace_packages_scalar(cfg.link_workspace_packages)),
        // `boolean | 'warn-only'` upstream; the default is `false`.
        (
            "scripts-prepend-node-path",
            scripts_prepend_node_path_scalar(cfg.scripts_prepend_node_path),
        ),
        ("node-linker", node_linker_scalar(cfg.node_linker)),
        ("resolution-mode", resolution_mode_scalar(cfg.resolution_mode)),
        ("catalog-mode", catalog_mode_scalar(cfg.catalog_mode)),
        ("save-catalog-name", save_catalog_name_scalar(cfg.save_catalog_name.as_deref())),
        ("fetch-retries", Int(i64::from(cfg.fetch_retries))),
        ("fetch-retry-factor", Int(i64::from(cfg.fetch_retry_factor))),
        ("fetch-retry-maxtimeout", Int(cfg.fetch_retry_maxtimeout as i64)),
        ("fetch-retry-mintimeout", Int(cfg.fetch_retry_mintimeout as i64)),
        ("fetch-timeout", Int(cfg.fetch_timeout as i64)),
        (
            "minimum-release-age",
            Int(cfg.minimum_release_age.expect("pacquet defaults minimum-release-age to Some")
                as i64),
        ),
        ("modules-cache-max-age", Int(cfg.modules_cache_max_age as i64)),
        ("dlx-cache-max-age", Int(cfg.dlx_cache_max_age as i64)),
        ("peers-suffix-max-length", Int(cfg.peers_suffix_max_length as i64)),
        (
            "hoist-pattern",
            Scalar::Set(cfg.hoist_pattern.clone().unwrap_or_default().into_iter().collect()),
        ),
        (
            "public-hoist-pattern",
            Scalar::Set(cfg.public_hoist_pattern.clone().unwrap_or_default().into_iter().collect()),
        ),
        ("git-shallow-hosts", Scalar::Set(cfg.git_shallow_hosts.iter().cloned().collect())),
    ]
}

fn node_linker_scalar(value: NodeLinker) -> Scalar {
    match value {
        NodeLinker::Isolated => s("isolated"),
        NodeLinker::Hoisted => s("hoisted"),
        NodeLinker::Pnp => s("pnp"),
    }
}

fn resolution_mode_scalar(value: ResolutionMode) -> Scalar {
    match value {
        ResolutionMode::Highest => s("highest"),
        ResolutionMode::TimeBased => s("time-based"),
        ResolutionMode::LowestDirect => s("lowest-direct"),
    }
}

fn catalog_mode_scalar(value: CatalogMode) -> Scalar {
    match value {
        CatalogMode::Manual => s("manual"),
        CatalogMode::Strict => s("strict"),
        CatalogMode::Prefer => s("prefer"),
    }
}

fn save_catalog_name_scalar(value: Option<&str>) -> Scalar {
    match value {
        Some(name) => s(name),
        None => Scalar::Undefined,
    }
}

fn link_workspace_packages_scalar(value: LinkWorkspacePackages) -> Scalar {
    match value {
        LinkWorkspacePackages::Off => Scalar::Bool(false),
        LinkWorkspacePackages::DirectOnly => Scalar::Bool(true),
        LinkWorkspacePackages::Deep => s("deep"),
    }
}

fn scripts_prepend_node_path_scalar(value: ScriptsPrependNodePath) -> Scalar {
    match value {
        ScriptsPrependNodePath::Never => Scalar::Bool(false),
        ScriptsPrependNodePath::Always => Scalar::Bool(true),
        ScriptsPrependNodePath::WarnOnly => s("warn-only"),
    }
}

/// The `defaultOptions` object literal, sliced out of pnpm's
/// config-reader source. Read live so the test tracks pnpm rather than
/// a checked-in copy that could silently drift.
fn read_pnpm_default_options() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../config/reader/src/index.ts");
    let src = std::fs::read_to_string(path).unwrap_or_else(|err| {
        panic!(
            "read pnpm config-reader source at {path}: {err}. \
             This contract test reads pnpm's `defaultOptions` from the TypeScript \
             tree; if config/reader moved, update the path.",
        )
    });
    let marker = "const defaultOptions: Partial<KebabCaseConfig> = {";
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("`{marker}` not found in config/reader/src/index.ts"));
    let body = &src[start + marker.len()..];
    // The block ends at the first line that is exactly the closing brace
    // at the object's indentation (`\n  }`).
    let end = body
        .find("\n  }")
        .expect("unterminated `defaultOptions` object literal in config/reader/src/index.ts");
    body[..end].to_string()
}

/// Every top-level key declared in the `defaultOptions` literal.
fn pnpm_keys(block: &str) -> BTreeSet<String> {
    block
        .lines()
        .filter_map(|line| {
            let line = strip_line_comment(line).trim();
            // A top-level entry is `key:` / `'key':` at the start of a
            // line. Array elements (`'github.com',`) have no `:` and are
            // skipped.
            let (key, _) = line.split_once(':')?;
            let key = key.trim().trim_matches('\'');
            // Keys are kebab-case identifiers; anything with a space or
            // quote left over is a value fragment from a multi-line
            // literal, not a key.
            (!key.is_empty() && key.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-'))
                .then(|| key.to_string())
        })
        .collect()
}

/// The raw value text pnpm assigns to `key`, with surrounding
/// whitespace, the trailing comma, and any `// ...` comment stripped.
/// Handles single-line values and `[ ... ]` arrays that span lines.
fn pnpm_raw_value<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let quoted = format!("'{key}':");
    let bare = format!("{key}:");
    let key_pos = block
        .match_indices(&quoted)
        .map(|(idx, mat)| (idx, mat.len()))
        .chain(block.match_indices(&bare).map(|(idx, mat)| (idx, mat.len())))
        // Only accept a match that sits at the start of a line (after
        // indentation) so a substring of a longer key can't match.
        .find(|&(idx, _)| {
            block[..idx].chars().rev().take_while(|&ch| ch != '\n').all(char::is_whitespace)
        })?;
    let after = block[key_pos.0 + key_pos.1..].trim_start();
    if after.starts_with('[') {
        let close = after.find(']').expect("unterminated array literal in defaultOptions");
        Some(&after[..=close])
    } else {
        let line_end = after.find('\n').unwrap_or(after.len());
        Some(strip_line_comment(&after[..line_end]).trim().trim_end_matches(','))
    }
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Parse a raw pnpm value into a [`Scalar`]. Panics on a shape this
/// test doesn't model (which only happens if a mapped key's value type
/// changed — exactly the drift worth failing on).
fn parse_scalar(raw: &str, key: &str) -> Scalar {
    let raw = raw.trim();
    if raw == "undefined" {
        return Scalar::Undefined;
    }
    if raw == "true" || raw == "false" {
        return Scalar::Bool(raw == "true");
    }
    if let Some(inner) = raw.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
        return Scalar::Str(inner.to_string());
    }
    if let Some(inner) = raw.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        // Strip comments per line *before* splitting on commas — a
        // comment line inside the array (e.g. the `git-shallow-hosts`
        // provenance note) shares no comma with the element below it.
        let items = inner
            .lines()
            .map(strip_line_comment)
            .flat_map(|line| line.split(','))
            .map(|item| item.trim().trim_matches('\''))
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect();
        return Scalar::Set(items);
    }
    // Integer, possibly with `_` digit separators and `*` products
    // (`24 * 60`, `7 * 24 * 60`).
    let product: Option<i64> = raw
        .split('*')
        .map(|factor| factor.trim().replace('_', "").parse::<i64>().ok())
        .try_fold(1_i64, |acc, factor| Some(acc * factor?));
    match product {
        Some(value) => Scalar::Int(value),
        None => panic!("pnpm default for {key:?} has an unsupported value shape: {raw:?}"),
    }
}

#[test]
fn pacquet_defaults_match_pnpm_cli_defaults() {
    let block = read_pnpm_default_options();
    let cfg = Config::default();

    for (key, pacquet_value) in mapped_rows(&cfg) {
        let raw = pnpm_raw_value(&block, key)
            .unwrap_or_else(|| panic!("pnpm `defaultOptions` has no entry for mapped key {key:?}"));
        let pnpm_value = parse_scalar(raw, key);
        assert_eq!(
            pacquet_value, pnpm_value,
            "default for {key:?} diverges from pnpm: pacquet={pacquet_value:?}, pnpm={pnpm_value:?} \
             (pnpm source: config/reader/src/index.ts)",
        );
    }
}

#[test]
fn every_pnpm_default_is_classified() {
    let block = read_pnpm_default_options();
    let pnpm_keys = pnpm_keys(&block);
    let cfg = Config::default();

    let mapped: BTreeSet<String> =
        mapped_rows(&cfg).into_iter().map(|(key, _)| key.to_string()).collect();
    let non_literal: BTreeSet<String> =
        NON_LITERAL.iter().map(std::string::ToString::to_string).collect();
    let not_ported: BTreeSet<String> =
        NOT_PORTED.iter().map(std::string::ToString::to_string).collect();

    // The three buckets must be disjoint — a key can't be both mapped
    // and skipped.
    for (a, b, label) in [
        (&mapped, &non_literal, "mapped ∩ non-literal"),
        (&mapped, &not_ported, "mapped ∩ not-ported"),
        (&non_literal, &not_ported, "non-literal ∩ not-ported"),
    ] {
        let overlap: Vec<_> = a.intersection(b).collect();
        assert!(overlap.is_empty(), "keys classified twice ({label}): {overlap:?}");
    }

    let classified: BTreeSet<String> = mapped
        .union(&non_literal)
        .cloned()
        .collect::<BTreeSet<_>>()
        .union(&not_ported)
        .cloned()
        .collect();

    let unclassified: Vec<_> = pnpm_keys.difference(&classified).collect();
    assert!(
        unclassified.is_empty(),
        "pnpm added settings pacquet hasn't classified: {unclassified:?}. \
         Port each (add a mapped row) or record it in NON_LITERAL / NOT_PORTED.",
    );

    let stale: Vec<_> = classified.difference(&pnpm_keys).collect();
    assert!(
        stale.is_empty(),
        "these keys are classified here but no longer in pnpm's `defaultOptions`: {stale:?}. \
         pnpm renamed or removed them; update the rows / skip lists.",
    );
}
