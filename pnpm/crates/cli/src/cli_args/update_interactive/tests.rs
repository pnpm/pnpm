use super::{InteractiveUpdateProject, collect_choices};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use serde_json::json;

const TEST_INTEGRITY: &str = "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==";

#[tokio::test]
async fn collects_choices_from_each_selected_workspace_importer() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let foo = manifest_with_dependency(temp.path(), "packages/a", "foo");
    let bar = manifest_with_dependency(temp.path(), "packages/b", "bar");
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      bar:
        specifier: ^1.0.0
        version: 1.0.0
",
    )
    .expect("parse workspace lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let foo_mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("foo", &registry))
        .expect(1)
        .create_async()
        .await;
    let bar_mock = server
        .mock("GET", "/bar")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("bar", &registry))
        .expect(1)
        .create_async()
        .await;
    let mut config = Config::new();
    config.registry = registry;
    let projects = [
        InteractiveUpdateProject { manifest: &foo, importer_id: "packages/a".to_string() },
        InteractiveUpdateProject { manifest: &bar, importer_id: "packages/b".to_string() },
    ];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &ThrottledClient::default(),
        false,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert_eq!(
        choices.iter().map(|choice| choice.alias.as_str()).collect::<Vec<_>>(),
        vec!["foo", "bar"],
    );
    // Each entry remembers the project it came from, which is what the
    // interactive list's `Workspace` column shows.
    assert_eq!(
        choices.iter().map(|choice| choice.workspace.as_deref()).collect::<Vec<_>>(),
        vec![Some("packages-a"), Some("packages-b")],
    );
    foo_mock.assert_async().await;
    bar_mock.assert_async().await;
}

#[tokio::test]
async fn keeps_distinct_aliases_for_the_same_package() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let direct = manifest_with_dependency_spec(temp.path(), "packages/a", ("foo", "^1.0.0"));
    let alias =
        manifest_with_dependency_spec(temp.path(), "packages/b", ("fooAlias", "npm:foo@^1.0.0"));
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      fooAlias:
        specifier: npm:foo@^1.0.0
        version: foo@1.0.0
",
    )
    .expect("parse workspace lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let foo_mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("foo", &registry))
        .expect(2)
        .create_async()
        .await;
    let mut config = Config::new();
    config.registry = registry;
    let projects = [
        InteractiveUpdateProject { manifest: &direct, importer_id: "packages/a".to_string() },
        InteractiveUpdateProject { manifest: &alias, importer_id: "packages/b".to_string() },
    ];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &ThrottledClient::default(),
        false,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert_eq!(
        choices.iter().map(|choice| choice.alias.as_str()).collect::<Vec<_>>(),
        vec!["foo", "fooAlias"],
    );
    foo_mock.assert_async().await;
}

/// The same dependency at the same version in two projects has to reach
/// [`super::choices::update_choices`] as two entries, or the collapsed
/// row it renders can only name one of the projects.
#[tokio::test]
async fn one_dependency_in_two_projects_keeps_both_workspaces() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let first = manifest_with_dependency(temp.path(), "packages/a", "foo");
    let second = manifest_with_dependency(temp.path(), "packages/b", "foo");
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
",
    )
    .expect("parse workspace lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let foo_mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("foo", &registry))
        .expect(2)
        .create_async()
        .await;
    let mut config = Config::new();
    config.registry = registry;
    let projects = [
        InteractiveUpdateProject { manifest: &first, importer_id: "packages/a".to_string() },
        InteractiveUpdateProject { manifest: &second, importer_id: "packages/b".to_string() },
    ];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &ThrottledClient::default(),
        false,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert_eq!(
        choices.iter().map(|choice| choice.workspace.as_deref()).collect::<Vec<_>>(),
        vec![Some("packages-a"), Some("packages-b")],
    );
    // And they render as one row naming both.
    let groups = super::choices::update_choices(&choices.iter().collect::<Vec<_>>(), true);
    assert!(
        groups[0].rows[1].label.contains("packages-a, packages-b"),
        "{}",
        groups[0].rows[1].label,
    );
    foo_mock.assert_async().await;
}

/// A project may omit its `name` or declare it empty; either way the
/// label would be blank, leaving several such projects indistinguishable
/// in the interactive list, so the entry falls back to the path that
/// identifies the project in the lockfile.
#[tokio::test]
async fn a_project_without_a_usable_name_is_labelled_with_its_importer_path() {
    for name in [None, Some(""), Some("   ")] {
        let temp = tempfile::tempdir().expect("create temporary workspace");
        let manifest = manifest_without_usable_name(temp.path(), "packages/a", name);
        let lockfile: Lockfile = serde_saphyr::from_str(
            r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
",
        )
        .expect("parse workspace lockfile");
        let mut server = mockito::Server::new_async().await;
        let registry = format!("{}/", server.url());
        let foo_mock = server
            .mock("GET", "/foo")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(package_body("foo", &registry))
            .expect(1)
            .create_async()
            .await;
        let mut config = Config::new();
        config.registry = registry;
        let projects = [InteractiveUpdateProject {
            manifest: &manifest,
            importer_id: "packages/a".to_string(),
        }];

        let choices = collect_choices(
            &projects,
            Some(&lockfile),
            &config,
            &ThrottledClient::default(),
            false,
            &[DependencyGroup::Prod],
        )
        .await
        .expect("collect interactive choices");

        assert_eq!(
            choices.iter().map(|choice| choice.workspace.as_deref()).collect::<Vec<_>>(),
            vec![Some("packages/a")],
            "a {name:?} name should fall back to the importer path",
        );
        foo_mock.assert_async().await;
    }
}

fn manifest_with_dependency(
    root: &std::path::Path,
    relative: &str,
    dependency: &str,
) -> PackageManifest {
    manifest_with_dependency_spec(root, relative, (dependency, "^1.0.0"))
}

fn manifest_with_dependency_spec(
    root: &std::path::Path,
    relative: &str,
    dependency: (&str, &str),
) -> PackageManifest {
    let (dependency, specifier) = dependency;
    let project_dir = root.join(relative);
    std::fs::create_dir_all(&project_dir).expect("create project directory");
    let manifest_path = project_dir.join("package.json");
    std::fs::write(
        &manifest_path,
        json!({
            "name": relative.replace('/', "-"),
            "dependencies": { dependency: specifier },
        })
        .to_string(),
    )
    .expect("write project manifest");
    PackageManifest::from_path(manifest_path).expect("read project manifest")
}

/// A project manifest whose `name` is absent, or present but empty —
/// both shapes a workspace project's `package.json` can carry.
fn manifest_without_usable_name(
    root: &std::path::Path,
    relative: &str,
    name: Option<&str>,
) -> PackageManifest {
    let project_dir = root.join(relative);
    std::fs::create_dir_all(&project_dir).expect("create project directory");
    let manifest_path = project_dir.join("package.json");
    let mut manifest = json!({ "dependencies": { "foo": "^1.0.0" } });
    if let Some(name) = name {
        manifest["name"] = json!(name);
    }
    std::fs::write(&manifest_path, manifest.to_string()).expect("write project manifest");
    PackageManifest::from_path(manifest_path).expect("read project manifest")
}

fn package_body(name: &str, registry: &str) -> String {
    let version = |version: &str| {
        json!({
            "name": name,
            "version": version,
            "dist": {
                "integrity": TEST_INTEGRITY,
                "tarball": format!("{registry}{name}/-/{name}-{version}.tgz"),
            },
        })
    };
    json!({
        "name": name,
        "dist-tags": { "latest": "1.1.0" },
        "versions": {
            "1.0.0": version("1.0.0"),
            "1.1.0": version("1.1.0"),
        },
    })
    .to_string()
}

mod selection {
    use super::super::{
        choices::{ChoiceGroup, ChoiceRow},
        flatten_groups, selected_packages,
    };

    fn group(message: &str, rows: &[(&str, Option<&str>)]) -> ChoiceGroup {
        ChoiceGroup {
            message: message.to_string(),
            rows: rows
                .iter()
                .map(|(label, value)| ChoiceRow {
                    label: (*label).to_string(),
                    value: value.map(str::to_string),
                })
                .collect(),
        }
    }

    /// Checking a group heading or a column header updates nothing —
    /// `dialoguer` lets either be checked, where pnpm's prompt disables
    /// them outright.
    #[test]
    fn headings_and_headers_select_nothing() {
        let groups =
            [group("dependencies", &[("Package Current", None), ("foo 1 ❯ 2", Some("foo"))])];

        let (labels, values) = flatten_groups(&groups);

        assert_eq!(labels.len(), 3, "heading, header row, and one package");
        // 0 is the heading, 1 the column header, 2 the package.
        assert_eq!(selected_packages(&values, &[0, 1]), Vec::<String>::new());
        assert_eq!(selected_packages(&values, &[2]), vec!["foo".to_string()]);
    }

    /// The same package offered by two importers is returned once, in the
    /// order it was first checked.
    #[test]
    fn a_package_checked_twice_is_returned_once() {
        let groups = [
            group("dependencies", &[("hdr", None), ("foo", Some("foo"))]),
            group("devDependencies", &[("hdr", None), ("bar", Some("bar")), ("foo", Some("foo"))]),
        ];

        let (_, values) = flatten_groups(&groups);

        // 2 = foo (prod), 5 = bar, 6 = foo (dev).
        assert_eq!(
            selected_packages(&values, &[2, 5, 6]),
            vec!["foo".to_string(), "bar".to_string()],
        );
    }

    /// An out-of-range index cannot panic the selection.
    #[test]
    fn an_unknown_index_is_ignored() {
        let groups = [group("dependencies", &[("hdr", None), ("foo", Some("foo"))])];

        let (_, values) = flatten_groups(&groups);

        assert_eq!(selected_packages(&values, &[99]), Vec::<String>::new());
    }
}
