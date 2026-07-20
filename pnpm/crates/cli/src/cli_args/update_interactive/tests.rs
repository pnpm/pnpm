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
