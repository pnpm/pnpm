#[cfg(unix)]
use super::render_recursive_json;
use super::{
    DependentProject, OutdatedDependencyOptions, OutdatedInWorkspace, OutdatedPackage, render_json,
    render_recursive_table, sort_outdated,
};
use crate::cli_args::outdated::{
    query::current_versions_from_importer,
    render::{Change, DEPENDENTS_COLUMN_WIDTH, classify, render_dependents, render_latest},
};
use node_semver::Version;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::DependencyGroup;
use std::{collections::HashMap, path::PathBuf};
#[cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};
use text_block_macros::text_block;

fn v(text: &str) -> Version {
    text.parse().expect("parse semver")
}

fn pkg(name: &str, current: &str, target: &str, group: DependencyGroup) -> OutdatedPackage {
    OutdatedPackage {
        alias: name.to_string(),
        package_name: name.to_string(),
        belongs_to: group,
        current: v(current),
        target: v(target),
        wanted: v(current),
        github_action: false,
        deprecated: None,
        homepage: None,
        workspace: None,
    }
}

// Mirrors `outdated() skips dependencies resolved from local refs` in
// `pnpm11/deps/inspection/outdated/test/outdated.spec.ts`: a dependency
// resolved to a local `link:`/`file:`/`workspace:` ref has no
// lockfile-pinned semver, so `collect_outdated` drops it before any
// registry fetch even when its manifest specifier is a plain semver range.
#[test]
fn current_versions_omit_local_refs() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    devDependencies:"
        "      private-workspace-pkg:"
        "        specifier: ^1.0.0"
        "        version: link:../private-workspace-pkg"
        "      injected-pkg:"
        "        specifier: ^1.0.0"
        "        version: file:../injected-pkg"
        "      workspace-pkg:"
        "        specifier: ^1.0.0"
        "        version: workspace:../workspace-pkg"
        "      is-positive:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse fixture lockfile");
    let versions = current_versions_from_importer(
        Some(&lockfile),
        Lockfile::ROOT_IMPORTER_KEY,
        &[DependencyGroup::Dev],
    );
    dbg!(&versions);
    assert_eq!(versions, HashMap::from([("is-positive".to_string(), v("1.0.0"))]));
}

#[test]
fn classify_detects_each_bump_kind() {
    assert_eq!(classify(&v("1.0.0"), &v("2.0.0")), Change::Breaking);
    assert_eq!(classify(&v("1.0.0"), &v("1.1.0")), Change::Feature);
    assert_eq!(classify(&v("1.0.0"), &v("1.0.1")), Change::Fix);
    assert_eq!(classify(&v("1.0.0"), &v("1.0.0")), Change::None);
    assert_eq!(classify(&v("1.0.0-alpha.1"), &v("1.0.0")), Change::Unknown);
}

#[test]
fn include_default_covers_all_three_groups() {
    let opts =
        OutdatedDependencyOptions { prod: false, dev: false, no_optional: false, optional: false };
    assert_eq!(
        opts.include(true),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn include_prod_keeps_dependencies_and_optional() {
    let opts =
        OutdatedDependencyOptions { prod: true, dev: false, no_optional: false, optional: false };
    assert_eq!(opts.include(true), vec![DependencyGroup::Prod, DependencyGroup::Optional]);
}

#[test]
fn include_dev_keeps_only_dev() {
    let opts =
        OutdatedDependencyOptions { prod: false, dev: true, no_optional: false, optional: false };
    assert_eq!(opts.include(true), vec![DependencyGroup::Dev]);
}

#[test]
fn include_no_optional_drops_optional() {
    let opts =
        OutdatedDependencyOptions { prod: false, dev: false, no_optional: true, optional: false };
    assert_eq!(opts.include(true), vec![DependencyGroup::Prod, DependencyGroup::Dev]);
}

#[test]
fn default_sort_orders_by_change_then_name() {
    let mut outdated = vec![
        pkg("breaking-z", "1.0.0", "2.0.0", DependencyGroup::Prod),
        pkg("fix-a", "1.0.0", "1.0.1", DependencyGroup::Prod),
        pkg("fix-b", "1.0.0", "1.0.1", DependencyGroup::Prod),
        pkg("feature-a", "1.0.0", "1.1.0", DependencyGroup::Prod),
    ];
    sort_outdated(&mut outdated, None);
    let order: Vec<&str> = outdated.iter().map(|item| item.package_name.as_str()).collect();
    assert_eq!(order, vec!["fix-a", "fix-b", "feature-a", "breaking-z"]);
}

#[test]
fn json_report_has_expected_shape() {
    let outdated = vec![pkg("foo", "1.0.0", "2.0.0", DependencyGroup::Dev)];
    let value: serde_json::Value =
        serde_json::from_str(&render_json(&outdated, false)).expect("valid JSON");
    let entry = &value["foo"];
    assert_eq!(entry["current"], "1.0.0");
    assert_eq!(entry["latest"], "2.0.0");
    assert_eq!(entry["wanted"], "1.0.0");
    assert_eq!(entry["isDeprecated"], false);
    assert_eq!(entry["dependencyType"], "devDependencies");
    assert!(entry.get("latestManifest").is_none(), "latestManifest is --long only");
}

// Ports `deps/inspection/commands/test/outdated/renderLatest.test.ts`.
// Colors are emitted only on a TTY, so the captured (non-TTY) output is
// plain text here.
#[test]
fn render_latest_outdated_and_deprecated() {
    let mut item = pkg("foo", "0.0.1", "1.0.0", DependencyGroup::Prod);
    item.deprecated = Some("This package is deprecated".to_string());
    let output = render_latest(&item);
    assert!(output.contains("1.0.0"), "shows the latest version: {output}");
    assert!(output.contains("(deprecated)"), "flags the deprecation: {output}");
}

#[test]
fn render_latest_outdated_and_not_deprecated() {
    let item = pkg("foo", "0.0.1", "1.0.0", DependencyGroup::Prod);
    let output = render_latest(&item);
    assert!(output.contains("1.0.0"), "shows the latest version: {output}");
    assert!(!output.contains("(deprecated)"), "no deprecation marker: {output}");
}

/// Visible column indices of the box-drawing characters that carry a
/// vertical stroke, ignoring ANSI SGR escapes so a colored cell only counts
/// its glyphs. These are the column boundaries a well-aligned table keeps
/// identical on every row.
fn border_columns(line: &str) -> Vec<usize> {
    const VERTICAL_BORDERS: &[char] = &['│', '┌', '┐', '├', '┤', '┼', '└', '┘', '┬', '┴'];
    let mut columns = Vec::new();
    let mut chars = line.chars();
    let mut column = 0;
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            skip_sgr_escape(&mut chars);
            continue;
        }
        if VERTICAL_BORDERS.contains(&ch) {
            columns.push(column);
        }
        column += 1;
    }
    columns
}

/// Step past the rest of an ANSI escape sequence, which ends at its
/// first alphabetic byte.
fn skip_sgr_escape(chars: &mut std::str::Chars<'_>) {
    for escape in chars.by_ref() {
        if escape.is_ascii_alphabetic() {
            break;
        }
    }
}

fn assert_borders_aligned(table: &str) {
    let mut rows = table.lines();
    let expected = rows.next().map(border_columns).unwrap_or_default();
    assert!(!expected.is_empty(), "expected box-drawing borders in:\n{table}");
    for row in table.lines() {
        assert_eq!(
            border_columns(row),
            expected,
            "every row must place its vertical borders at the same columns:\n{table}",
        );
    }
}

// Enabling `tabled`'s `ansi` feature makes it size a colored cell by its
// visible glyphs, not its escape bytes; without it the columns and borders
// drift apart. Feed a modern-style table the kind of ANSI-colored cells the
// `outdated` renderers emit — a highlighted version segment, a dim group
// label, bright-blue headers — and confirm every vertical border lands on the
// same column. Coloring the cells directly (`owo_colors` unconditional
// styling) rather than forcing the global color override keeps the test free
// of process-global state, so it can't leak into or be perturbed by other
// tests running in the same binary.
#[test]
fn colored_table_borders_stay_aligned() {
    use owo_colors::OwoColorize;
    use tabled::{builder::Builder, settings::Style};

    let header = ["Package", "Current", "Latest"].map(|name| name.bright_blue().to_string());
    let rows = [
        [
            format!("actions/checkout {}", "(github action)".dimmed()),
            "7.0.0".to_owned(),
            format!("7.0.{}", "1".green()),
        ],
        [
            format!("@typescript/native-preview {}", "(dev)".dimmed()),
            "1.0.0".to_owned(),
            "26.1.1".red().to_string(),
        ],
    ];

    let mut builder = Builder::default();
    builder.push_record(header);
    for row in rows {
        builder.push_record(row);
    }
    let mut table = builder.build();
    table.with(Style::modern());

    assert_borders_aligned(&table.to_string());
}

#[test]
fn json_report_long_includes_latest_manifest() {
    let mut item = pkg("foo", "1.0.0", "2.0.0", DependencyGroup::Prod);
    item.deprecated = Some("do not use".to_string());
    item.homepage = Some("https://example.com".to_string());
    let value: serde_json::Value =
        serde_json::from_str(&render_json(&[item], true)).expect("valid JSON");
    let manifest = &value["foo"]["latestManifest"];
    assert_eq!(manifest["version"], "2.0.0");
    assert_eq!(manifest["deprecated"], "do not use");
    assert_eq!(manifest["homepage"], "https://example.com");
    assert_eq!(value["foo"]["isDeprecated"], true);
}

#[test]
fn dependent_names_are_sanitized_for_terminal_output() {
    let entry = OutdatedInWorkspace {
        package: pkg("foo", "1.0.0", "2.0.0", DependencyGroup::Prod),
        dependents: vec![DependentProject {
            name: "app\n\t\u{1b}[2J".to_string(),
            location: PathBuf::from("packages/app"),
        }],
    };

    assert_eq!(render_dependents(&entry), "app[2J");
}

// Mirrors the `getCellWidth(data, 3, 30)` clamp of pnpm 11's recursive
// renderer in `pnpm11/deps/inspection/commands/src/outdated/recursive.ts`.
// A dependency shared by a dozen workspace projects lists all of them in one
// cell, which sizes the `Dependents` column past any terminal unless the cell
// wraps.
#[test]
fn recursive_table_wraps_the_dependents_column() {
    // The last dependent is one unbreakable name longer than the clamp, so the
    // rendered column lands on exactly `DEPENDENTS_COLUMN_WIDTH` rather than on
    // whatever width the shorter names happen to pack into.
    let long_name = "example-workspace-package-with-a-name-past-the-clamp";
    assert!(long_name.len() > DEPENDENTS_COLUMN_WIDTH);
    let entry = OutdatedInWorkspace {
        package: pkg("is-odd", "3.0.0", "3.0.1", DependencyGroup::Prod),
        dependents: (1..=12)
            .map(|index| DependentProject {
                name: format!("example-workspace-package-{index:02}"),
                location: PathBuf::from(format!("packages/pkg-{index:02}")),
            })
            .chain([DependentProject {
                name: long_name.to_string(),
                location: PathBuf::from("packages/pkg-long"),
            }])
            .collect(),
    };

    let table = render_recursive_table(&[entry], false);
    println!("{table}");
    assert_borders_aligned(&table);
    assert_eq!(last_column_width(&table), DEPENDENTS_COLUMN_WIDTH);

    let cells = last_column_cells(&table);
    let (heading, wrapped) = cells.split_first().expect("a heading and one row");
    assert_eq!(*heading, "Dependents");
    assert!(wrapped.len() > 1, "the dependents cell must wrap onto several lines");

    let rejoined = wrapped.concat();
    for index in 1..=12 {
        let name = format!("example-workspace-package-{index:02}");
        assert!(rejoined.contains(&name), "wrapping must not drop {name}");
    }
    assert!(rejoined.contains(long_name), "wrapping must not drop {long_name}");
}

fn last_column_cells(table: &str) -> Vec<&str> {
    table.lines().filter_map(|line| line.rsplit('│').nth(1)).map(str::trim).collect()
}

/// Content width of the table's rightmost column, excluding its border and
/// padding.
fn last_column_width(table: &str) -> usize {
    const PADDING: usize = 2;
    let borders = border_columns(table.lines().next().expect("top border"));
    let [.., left, right] = borders[..] else {
        panic!("expected at least two column boundaries in:\n{table}");
    };
    right - left - 1 - PADDING
}

#[cfg(unix)]
#[test]
fn recursive_json_replaces_invalid_utf8_in_locations() {
    let entry = OutdatedInWorkspace {
        package: pkg("foo", "1.0.0", "2.0.0", DependencyGroup::Prod),
        dependents: vec![DependentProject {
            name: "app".to_string(),
            location: PathBuf::from(OsString::from_vec(b"packages/\xff-app".to_vec())),
        }],
    };

    let value: serde_json::Value =
        serde_json::from_str(&render_recursive_json(&[entry], false)).expect("valid JSON");
    assert_eq!(value["foo"]["dependentPackages"][0]["location"], "packages/�-app");
}
