use super::{
    environment::{accept_server_lockfile, resolve_via_pnpr},
    manifest::{Manifest, ProjectPackage},
};
use pnpm_config::Config;
use pnpm_python_resolver::Target;
use std::path::Path;

fn target() -> Target {
    let environment = serde_json::from_value(serde_json::json!({
        "implementation_name": "cpython",
        "implementation_version": "3.12.0",
        "os_name": "posix",
        "platform_machine": "x86_64",
        "platform_release": "6.1.0",
        "platform_system": "Linux",
        "platform_version": "#1 SMP",
        "python_full_version": "3.12.0",
        "platform_python_implementation": "CPython",
        "python_version": "3.12",
        "sys_platform": "linux",
    }))
    .expect("marker environment fixture");
    Target { environment, tags: vec!["py3-none-any".to_string()] }
}

fn handshake_body(ecosystems: &[&str]) -> String {
    serde_json::json!({
        "pnpr": { "versions": [0], "artifacts": [], "fixLockfile": [0], "ecosystems": ecosystems },
    })
    .to_string()
}

fn config_for_pnpr(server: &str) -> Config {
    let mut config = Config::new();
    config.pnpr_server = Some(server.to_string());
    config
}

fn requirements(specifiers: &[&str]) -> Vec<pep508_rs::Requirement> {
    specifiers
        .iter()
        .map(|requirement| pnpm_python_resolver::parse_requirement(requirement))
        .collect::<miette::Result<Vec<_>>>()
        .expect("requirement fixtures")
}

fn server_lockfile(index: &str) -> serde_json::Value {
    serde_json::json!({
        "lock-version": "1.0",
        "created-by": "pnpm",
        "environments": ["sys_platform == 'linux'"],
        "packages": [{
            "name": "demo",
            "version": "1.0.0",
            "wheels": [{
                "name": "demo-1.0.0-py3-none-any.whl",
                "url": format!("{index}demo-1.0.0-py3-none-any.whl"),
                "hashes": { "sha256": "a".repeat(64) },
            }],
        }],
        "tool": { "pnpm": {
            "requirements": ["demo"],
            "environment": target().environment,
            "tags": target().tags,
            "index": index,
        } },
    })
}

#[tokio::test]
async fn python_resolution_is_offloaded_to_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let index = "https://index.example.test/simple/";
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "pypi"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "ecosystem": "pypi",
            "requirements": ["demo"],
            "index": index,
        })))
        .with_header("content-type", "application/x-ndjson")
        .with_body(format!(
            "{}\n",
            serde_json::json!({ "type": "done", "lockfile": server_lockfile(index) }),
        ))
        .expect(1)
        .create_async()
        .await;

    let resolved = resolve_via_pnpr(
        &config_for_pnpr(&server.url()),
        &requirements(&["demo"]),
        &target(),
        index,
        None,
    )
    .await
    .unwrap()
    .expect("the server resolves Python");

    assert_eq!(resolved.packages[0].name.as_ref(), "demo");
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn a_server_without_python_support_leaves_resolution_local() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .expect(0)
        .create_async()
        .await;

    let resolved = resolve_via_pnpr(
        &config_for_pnpr(&server.url()),
        &requirements(&["demo"]),
        &target(),
        "https://index.example.test/simple/",
        None,
    )
    .await
    .unwrap();

    assert!(resolved.is_none());
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn an_offline_install_does_not_reach_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .expect(0)
        .create_async()
        .await;
    let mut config = config_for_pnpr(&server.url());
    config.offline = true;

    let resolved = resolve_via_pnpr(
        &config,
        &requirements(&["demo"]),
        &target(),
        "https://index.example.test/simple/",
        None,
    )
    .await
    .unwrap();

    assert!(resolved.is_none());
    handshake.assert_async().await;
}

#[test]
fn a_lockfile_answering_another_question_is_refused() {
    let index = "https://index.example.test/simple/";
    let inputs = pnpm_python_resolver::Inputs::new(&requirements(&["demo"]), &target(), index);
    let answered: pnpm_python_resolver::Lockfile =
        serde_json::from_value(server_lockfile(index)).expect("lockfile fixture");

    accept_server_lockfile(&answered, &inputs, None).expect("the same question");

    let other_requirements =
        pnpm_python_resolver::Inputs::new(&requirements(&["demo", "extra"]), &target(), index);
    let error = accept_server_lockfile(&answered, &other_requirements, None)
        .expect_err("other requirements");
    assert!(error.to_string().contains("for other inputs"), "{error}");
    let error = accept_server_lockfile(&answered, &inputs, Some(">=3.12"))
        .expect_err("another requires-python");
    assert!(error.to_string().contains("for other inputs"), "{error}");
}

fn project_package(root: &Path, contents: &str) -> ProjectPackage {
    Manifest::parse(contents)
        .expect("manifest fixture")
        .project_package(root)
        .expect("the project package")
}

fn own_package(root: &Path, contents: &str) -> serde_json::Value {
    match project_package(root, contents) {
        ProjectPackage::Installable(package) => {
            serde_json::to_value(*package).expect("serialize the project package")
        }
        ProjectPackage::Virtual => panic!("the manifest declares a package"),
        ProjectPackage::DynamicVersion => panic!("the manifest declares a version"),
    }
}

fn packaged(body: &str) -> String {
    format!(
        "[project]\nname = 'My.App'\nversion = '1.0+Local.1'\n{body}\n[build-system]\nrequires = ['hatchling']\n"
    )
}

#[test]
fn the_projects_own_package_is_named_and_described_as_an_installed_distribution_is() {
    let root = tempfile::tempdir().expect("project directory");
    let package = own_package(
        root.path(),
        &packaged(
            "requires-python = '>=3.10'\ndependencies = ['alpha >= 1']\n\n[project.scripts]\nmy-app = 'my_app:main'\n\n[project.entry-points.pytest11]\nmy-app = 'my_app.plugin'\n",
        ),
    );
    dbg!(&package);
    assert_eq!(package["dist_info"], "my_app-1.0_local.1.dist-info");
    assert_eq!(package["pth"], "__pnpm__my_app.pth");
    assert_eq!(
        package["directory"],
        root.path()
            .to_str()
            .expect("a printable path")
    );
    assert_eq!(
        package["metadata"],
        "Metadata-Version: 2.1\nName: my-app\nVersion: 1.0+local.1\nRequires-Python: >=3.10\nRequires-Dist: alpha>=1\n",
    );
    assert_eq!(package["entry_points"]["console_scripts"]["my-app"], "my_app:main");
    assert_eq!(package["entry_points"]["pytest11"]["my-app"], "my_app.plugin");
}

#[test]
fn the_build_backends_package_directories_are_where_the_projects_modules_are_imported_from() {
    let root = tempfile::tempdir().expect("project directory");
    let flat = own_package(root.path(), &packaged("dependencies = []\n"));
    assert_eq!(flat["paths"], serde_json::json!([root.path()]));

    std::fs::create_dir(root.path().join("src")).expect("a src layout");
    let src = own_package(root.path(), &packaged("dependencies = []\n"));
    assert_eq!(src["paths"], serde_json::json!([root.path().join("src")]));

    let hatch = own_package(
        root.path(),
        &packaged(
            "dependencies = []\n\n[tool.hatch.build.targets.wheel]\npackages = ['modules/one', 'modules/two']\n",
        ),
    );
    assert_eq!(hatch["paths"], serde_json::json!([root.path().join("modules")]));

    let setuptools = own_package(
        root.path(),
        &packaged("dependencies = []\n\n[tool.setuptools]\npackage-dir = { '' = 'lib' }\n"),
    );
    assert_eq!(setuptools["paths"], serde_json::json!([root.path().join("lib")]));
}

#[test]
fn only_a_project_a_build_backend_builds_has_a_package_to_install() {
    let root = tempfile::tempdir().expect("project directory");
    let backend_only = "[project]\nname = 'app'\nversion = '1.0'\ndependencies = []\n";
    assert!(matches!(project_package(root.path(), backend_only), ProjectPackage::Virtual,));
    assert!(matches!(
        project_package(root.path(), &format!("{backend_only}\n[tool.uv]\npackage = true\n")),
        ProjectPackage::Installable(_),
    ));
    assert!(matches!(
        project_package(root.path(), &packaged("dependencies = []\n")),
        ProjectPackage::Installable(_),
    ));
    assert!(matches!(
        project_package(
            root.path(),
            &format!("{}\n[tool.uv]\npackage = false\n", packaged("dependencies = []\n"))
        ),
        ProjectPackage::Virtual,
    ));
    assert!(matches!(
        project_package(
            root.path(),
            "[project]\nname = 'app'\ndynamic = ['version']\ndependencies = []\n[build-system]\nrequires = ['hatchling']\n",
        ),
        ProjectPackage::DynamicVersion,
    ));
}
