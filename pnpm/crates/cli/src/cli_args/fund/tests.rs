use super::{
    funding::{FundingSource, funding_sources, is_valid_funding, normalize_funding},
    human::render_human,
    open::open_package_funding,
    projects::ProjectDependencies,
    report::FundingReport,
};
use crate::cli_args::deps_tree::{DependencyNode, DependencyPackage};
use pnpm_network_web_auth::OpenUrl;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{cell::RefCell, fs, io, num::NonZeroUsize, path::Path};

thread_local! {
    static OPENED_URLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// [`OpenUrl`] fake standing in for the user's browser.
struct RecordingBrowser;

impl OpenUrl for RecordingBrowser {
    fn open_url(url: &str) -> io::Result<()> {
        OPENED_URLS.with(|urls| urls.borrow_mut().push(url.to_owned()));
        Ok(())
    }
}

fn opened_urls() -> Vec<String> {
    OPENED_URLS.with(RefCell::take)
}

/// A tree node whose package directory holds `manifest`.
fn installed(root: &Path, manifest: &Value, dependencies: Vec<DependencyNode>) -> DependencyNode {
    let name = manifest["name"].as_str().expect("fixture manifest has a name");
    let version = manifest["version"].as_str().expect("fixture manifest has a version");
    let dir = root.join(format!("{name}@{version}"));
    fs::create_dir_all(&dir).expect("create package dir");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package manifest");
    DependencyNode {
        alias: name.to_string(),
        dependencies,
        package: DependencyPackage {
            name: name.to_string(),
            version: version.to_string(),
            path: dir.to_string_lossy().into_owned(),
            ..DependencyPackage::default()
        },
        ..DependencyNode::default()
    }
}

fn project(root: &Path, manifest: Value, dependencies: Vec<DependencyNode>) -> ProjectDependencies {
    ProjectDependencies { dir: root.join("project"), manifest: Some(manifest), dependencies }
}

/// `a` and `c` share a funding URL, and `b`, a dependency of `a`, lists
/// two sources. The expected outputs below are what `npm fund` prints for
/// the same tree.
fn shared_url_project(root: &Path) -> ProjectDependencies {
    let sponsored = installed(
        root,
        &json!({
            "name": "b",
            "version": "2.0.0",
            "funding": [{ "type": "github", "url": "https://github.com/sponsors/b" }, "https://example.com/b"],
        }),
        Vec::new(),
    );
    let shared_parent = installed(
        root,
        &json!({ "name": "a", "version": "1.0.0", "funding": "https://example.com/shared" }),
        vec![sponsored],
    );
    let shared_sibling = installed(
        root,
        &json!({
            "name": "c",
            "version": "3.0.0",
            "funding": { "type": "opencollective", "url": "https://example.com/shared" },
        }),
        Vec::new(),
    );
    project(
        root,
        json!({ "name": "root", "version": "1.0.0" }),
        vec![shared_parent, shared_sibling],
    )
}

#[test]
fn funding_accepts_only_http_urls_with_a_host() {
    assert!(is_valid_funding(&json!("https://example.com/fund")));
    assert!(is_valid_funding(&json!({ "type": "patreon", "url": "http://example.com" })));
    assert!(is_valid_funding(
        &json!(["https://example.com/a", { "url": "https://example.com/b" }])
    ));
    assert!(!is_valid_funding(&json!("ftp://example.com/fund")));
    assert!(!is_valid_funding(&json!("not a url")));
    assert!(!is_valid_funding(&json!({ "type": "patreon" })));
    assert!(!is_valid_funding(&json!(["https://example.com/a", "mailto:fund@example.com"])));
    assert!(!is_valid_funding(&json!([])));
    assert!(!is_valid_funding(&json!(42)));
}

#[test]
fn funding_normalizes_string_shorthands() {
    assert_eq!(
        normalize_funding(&json!("https://example.com")),
        json!({ "url": "https://example.com" }),
    );
    assert_eq!(
        normalize_funding(
            &json!(["https://example.com/a", { "type": "github", "url": "https://example.com/b" }])
        ),
        json!([{ "url": "https://example.com/a" }, { "type": "github", "url": "https://example.com/b" }]),
    );
    assert_eq!(
        funding_sources(
            &json!(["mailto:x@example.com", { "type": "github", "url": "https://example.com/b" }])
        ),
        [FundingSource { kind: Some("github"), url: "https://example.com/b" }],
    );
}

#[test]
fn report_matches_npm_fund_json() {
    let root = tempfile::tempdir().expect("create temp dir");
    let report = FundingReport::build(&shared_url_project(root.path()));
    let report = serde_json::to_value(&report).expect("serialize report");
    assert_eq!(
        report,
        json!({
            "length": 3,
            "name": "root",
            "version": "1.0.0",
            "dependencies": {
                "a": {
                    "version": "1.0.0",
                    "funding": { "url": "https://example.com/shared" },
                    "dependencies": {
                        "b": {
                            "version": "2.0.0",
                            "funding": [
                                { "type": "github", "url": "https://github.com/sponsors/b" },
                                { "url": "https://example.com/b" },
                            ],
                        },
                    },
                },
                "c": {
                    "version": "3.0.0",
                    "funding": { "type": "opencollective", "url": "https://example.com/shared" },
                },
            },
        }),
    );
}

#[test]
fn report_lists_each_package_once_at_its_shallowest_level() {
    let root = tempfile::tempdir().expect("create temp dir");
    let funded = |name: &str| json!({ "name": name, "version": "1.0.0", "funding": format!("https://example.com/{name}") });
    let deep = installed(root.path(), &funded("deep"), Vec::new());
    let unfunded =
        installed(root.path(), &json!({ "name": "unfunded", "version": "1.0.0" }), vec![deep]);
    let shared = installed(root.path(), &funded("shared"), Vec::new());
    let first = installed(root.path(), &funded("first"), vec![shared.clone()]);
    let project = project(root.path(), json!({ "name": "root" }), vec![first, unfunded, shared]);

    let report = serde_json::to_value(FundingReport::build(&project)).expect("serialize report");

    assert_eq!(
        report,
        json!({
            "length": 3,
            "name": "root",
            "dependencies": {
                "first": { "version": "1.0.0", "funding": { "url": "https://example.com/first" } },
                "shared": { "version": "1.0.0", "funding": { "url": "https://example.com/shared" } },
                "deep": { "version": "1.0.0", "funding": { "url": "https://example.com/deep" } },
            },
        }),
    );
}

#[test]
fn human_output_matches_npm_fund() {
    let root = tempfile::tempdir().expect("create temp dir");
    let report = FundingReport::build(&shared_url_project(root.path()));
    let expected = [
        "root@1.0.0",
        "└─┬ https://example.com/shared",
        "  │ └── a@1.0.0, c@3.0.0",
        "  └── https://github.com/sponsors/b",
        "      └── b@2.0.0",
        "",
    ];
    assert_eq!(render_human(&report), expected.join("\n"));
}

#[test]
fn human_output_groups_siblings_under_a_shared_url() {
    let root = tempfile::tempdir().expect("create temp dir");
    let manifest =
        |name: &str, url: &str| json!({ "name": name, "version": "1.0.0", "funding": url });
    let dependencies = vec![
        installed(
            root.path(),
            &manifest("call-bind", "https://github.com/sponsors/ljharb"),
            Vec::new(),
        ),
        installed(
            root.path(),
            &manifest("chalk", "https://github.com/chalk/chalk?sponsor=1"),
            Vec::new(),
        ),
        installed(root.path(), &manifest("gopd", "https://github.com/sponsors/ljharb"), Vec::new()),
    ];
    let report = FundingReport::build(&project(
        root.path(),
        json!({ "name": "root", "version": "1.0.0" }),
        dependencies,
    ));
    let expected = [
        "root@1.0.0",
        "├── https://github.com/sponsors/ljharb",
        "│   └── call-bind@1.0.0, gopd@1.0.0",
        "└── https://github.com/chalk/chalk?sponsor=1",
        "    └── chalk@1.0.0",
        "",
    ];
    assert_eq!(render_human(&report), expected.join("\n"));
}

#[test]
fn open_package_funding_opens_the_only_source() {
    let root = tempfile::tempdir().expect("create temp dir");
    let projects = [shared_url_project(root.path())];
    open_package_funding::<RecordingBrowser>("a", None, root.path(), &projects).expect("open a");
    assert_eq!(opened_urls(), ["https://example.com/shared"]);
}

#[test]
fn open_package_funding_picks_a_source_with_which() {
    let root = tempfile::tempdir().expect("create temp dir");
    let projects = [shared_url_project(root.path())];

    open_package_funding::<RecordingBrowser>("b@2", None, root.path(), &projects).expect("list b");
    assert_eq!(opened_urls(), Vec::<String>::new());

    let second = NonZeroUsize::new(2);
    open_package_funding::<RecordingBrowser>("b", second, root.path(), &projects).expect("open b");
    assert_eq!(opened_urls(), ["https://example.com/b"]);

    let out_of_range = NonZeroUsize::new(3);
    open_package_funding::<RecordingBrowser>("b", out_of_range, root.path(), &projects)
        .expect("list b");
    assert_eq!(opened_urls(), Vec::<String>::new());
}

#[test]
fn open_package_funding_uses_the_newest_installed_version() {
    let root = tempfile::tempdir().expect("create temp dir");
    let manifest = |version: &str| json!({ "name": "dup", "version": version, "funding": format!("https://example.com/{version}") });
    let old = installed(root.path(), &manifest("1.0.0"), Vec::new());
    let new = installed(root.path(), &manifest("1.10.0"), Vec::new());
    let wrapper =
        installed(root.path(), &json!({ "name": "wrapper", "version": "1.0.0" }), vec![new]);
    let projects = [project(root.path(), json!({ "name": "root" }), vec![old, wrapper])];

    open_package_funding::<RecordingBrowser>("dup", None, root.path(), &projects)
        .expect("open dup");
    assert_eq!(opened_urls(), ["https://example.com/1.10.0"]);
}

#[test]
fn open_package_funding_reads_a_directory() {
    let root = tempfile::tempdir().expect("create temp dir");
    fs::write(root.path().join("package.json"), r#"{"funding":"https://example.com/here"}"#)
        .expect("write package.json");
    open_package_funding::<RecordingBrowser>(".", None, root.path(), &[]).expect("open .");
    assert_eq!(opened_urls(), ["https://example.com/here"]);
}

#[test]
fn funding_urls_are_printed_and_opened_without_credentials() {
    let root = tempfile::tempdir().expect("create temp dir");
    let manifest = json!({ "name": "secret", "version": "1.0.0", "funding": "https://user:token@example.com/fund" });
    let projects = [project(
        root.path(),
        json!({ "name": "root", "version": "1.0.0" }),
        vec![installed(root.path(), &manifest, Vec::new())],
    )];

    let expected = ["root@1.0.0", "└── https://example.com/fund", "    └── secret@1.0.0", ""];
    assert_eq!(render_human(&FundingReport::build(&projects[0])), expected.join("\n"));
    open_package_funding::<RecordingBrowser>("secret", None, root.path(), &projects)
        .expect("open secret");
    assert_eq!(opened_urls(), ["https://example.com/fund"]);
}

#[test]
fn open_package_funding_fails_without_a_valid_source() {
    let root = tempfile::tempdir().expect("create temp dir");
    let unfunded =
        installed(root.path(), &json!({ "name": "plain", "version": "1.0.0" }), Vec::new());
    let projects = [project(root.path(), json!({ "name": "root" }), vec![unfunded])];

    for spec in ["plain", "missing"] {
        let error = open_package_funding::<RecordingBrowser>(spec, None, root.path(), &projects)
            .expect_err("no funding to open");
        assert_eq!(error.to_string(), format!("No valid funding method available for: {spec}"));
    }
    assert_eq!(opened_urls(), Vec::<String>::new());
}
