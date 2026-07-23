use super::{
    Change, DependentProject, OutdatedDependencyOptions, OutdatedInWorkspace, OutdatedPackage,
    PackumentCache, classify, current_versions_from_importer, fetch_package_cached,
    render_dependents, render_json, render_latest, render_recursive_json, render_recursive_table,
    render_table, sort_outdated,
};
use node_semver::Version;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::DependencyGroup;
use std::{collections::HashMap, path::PathBuf};
use text_block_macros::text_block;

#[cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

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
    let opts = OutdatedDependencyOptions { prod: false, dev: false, no_optional: false };
    assert_eq!(
        opts.include(),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn include_prod_keeps_dependencies_and_optional() {
    let opts = OutdatedDependencyOptions { prod: true, dev: false, no_optional: false };
    assert_eq!(opts.include(), vec![DependencyGroup::Prod, DependencyGroup::Optional]);
}

#[test]
fn include_dev_keeps_only_dev() {
    let opts = OutdatedDependencyOptions { prod: false, dev: true, no_optional: false };
    assert_eq!(opts.include(), vec![DependencyGroup::Dev]);
}

#[test]
fn include_no_optional_drops_optional() {
    let opts = OutdatedDependencyOptions { prod: false, dev: false, no_optional: true };
    assert_eq!(opts.include(), vec![DependencyGroup::Prod, DependencyGroup::Dev]);
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

/// Display width of `line`, ignoring ANSI SGR escape sequences.
fn visible_width(line: &str) -> usize {
    let mut chars = line.chars();
    let mut width = 0;
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for escape_ch in chars.by_ref() {
                if escape_ch == 'm' {
                    break;
                }
            }
        } else {
            width += 1;
        }
    }
    width
}

fn assert_borders_aligned(table: &str) {
    let widths: Vec<usize> = table.lines().map(visible_width).collect();
    assert!(
        widths.windows(2).all(|pair| pair[0] == pair[1]),
        "every row must have the same display width so the borders line up, got {widths:?} for:\n{table}"
    );
}

// A colored cell carries ANSI escape sequences whose bytes must not count
// toward the column width, or the borders drift out of alignment. Force
// color on and check that the box-drawing borders stay vertically aligned
// across rows whose cells differ in how many escapes they hold.
#[test]
fn colored_table_borders_stay_aligned() {
    owo_colors::set_override(true);

    let packages = [
        pkg("actions/checkout", "7.0.0", "7.0.1", DependencyGroup::Dev),
        pkg("typescript", "6.0.3", "7.0.2", DependencyGroup::Dev),
        pkg("@typescript/native-preview", "1.0.0", "26.1.1", DependencyGroup::Dev),
    ];
    assert_borders_aligned(&render_table(&packages, false));

    let workspace = [OutdatedInWorkspace {
        package: pkg("typescript", "6.0.3", "7.0.2", DependencyGroup::Dev),
        dependents: vec![DependentProject {
            name: "app".to_string(),
            location: PathBuf::from("packages/app"),
        }],
    }];
    assert_borders_aligned(&render_recursive_table(&workspace, false));

    owo_colors::unset_override();
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

#[tokio::test]
async fn packument_cache_deduplicates_concurrent_fetches() {
    let mut server = mockito::Server::new_async().await;
    let package = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{ "name": "foo", "dist-tags": {}, "versions": {} }"#)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let config = Config::new();
    let client = ThrottledClient::default();
    let cache = PackumentCache::default();
    let first_fetch = fetch_package_cached(&cache, "foo", &client, &registry, &config.auth_headers);
    let second_fetch =
        fetch_package_cached(&cache, "foo", &client, &registry, &config.auth_headers);

    let (first, second) = tokio::join!(first_fetch, second_fetch);

    assert_eq!(first.expect("first fetch").name, "foo");
    assert_eq!(second.expect("second fetch").name, "foo");
    package.assert_async().await;
}

#[tokio::test]
async fn packument_cache_does_not_memoize_failures() {
    let mut server = mockito::Server::new_async().await;
    let failed_request = server
        .mock("GET", "/foo")
        .with_status(500)
        .with_body("not package metadata")
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let config = Config::new();
    let client = ThrottledClient::default();
    let cache = PackumentCache::default();

    assert!(
        fetch_package_cached(&cache, "foo", &client, &registry, &config.auth_headers)
            .await
            .is_err(),
    );
    failed_request.assert_async().await;
    failed_request.remove_async().await;

    let successful_request = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{ "name": "foo", "dist-tags": {}, "versions": {} }"#)
        .expect(1)
        .create_async()
        .await;

    let package = fetch_package_cached(&cache, "foo", &client, &registry, &config.auth_headers)
        .await
        .expect("retry package fetch");
    assert_eq!(package.name, "foo");
    successful_request.assert_async().await;
}

#[tokio::test]
async fn packument_cache_recovers_from_poisoning() {
    let mut server = mockito::Server::new_async().await;
    let package = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{ "name": "foo", "dist-tags": {}, "versions": {} }"#)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let config = Config::new();
    let client = ThrottledClient::default();
    let cache = PackumentCache::default();

    std::thread::scope(|scope| {
        assert!(
            scope
                .spawn(|| {
                    let _guard = cache.lock().expect("lock packument cache");
                    panic!("poison packument cache");
                })
                .join()
                .is_err(),
        );
    });

    let fetched = fetch_package_cached(&cache, "foo", &client, &registry, &config.auth_headers)
        .await
        .expect("fetch package after cache poisoning");
    assert_eq!(fetched.name, "foo");
    package.assert_async().await;
}
