use super::super::{project, python, serve, wheel};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use std::fs;

#[tokio::test]
async fn rejected_releases_cannot_replace_index_dependencies_with_direct_wheels() {
    for conflict_in_source in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let url = format!("{}/helper-1.0-py3-none-any.whl", server.url());
        let source_metadata = if conflict_in_source { "Requires-Dist: conflict>=2\n" } else { "" };
        let artifact = server
            .mock("GET", "/helper-1.0-py3-none-any.whl")
            .with_body(wheel(
                "helper",
                "1.0",
                source_metadata,
                &[("helper/origin.py", "SOURCE = 'direct'\n")],
            ))
            .expect(1)
            .create_async()
            .await;
        let conflict = if conflict_in_source { "" } else { "Requires-Dist: conflict>=2\n" };
        let _alpha = serve(
            &mut server,
            "alpha",
            &[
                (
                    "2.0",
                    wheel(
                        "alpha",
                        "2.0",
                        &format!("Requires-Dist: helper @ {url}\n{conflict}"),
                        &[],
                    ),
                ),
                ("1.0", wheel("alpha", "1.0", "Requires-Dist: helper>=1\n", &[])),
            ],
        )
        .await;
        let _helper = serve(
            &mut server,
            "helper",
            &[("1.0", wheel("helper", "1.0", "", &[("helper/origin.py", "SOURCE = 'index'\n")]))],
        )
        .await;
        let _conflict = serve(
            &mut server,
            "conflict",
            &[
                ("1.0", wheel("conflict", "1.0", "", &[])),
                ("2.0", wheel("conflict", "2.0", "", &[])),
            ],
        )
        .await;
        project(root.path(), &server.url(), &["alpha", "conflict<2"]);
        pacquet_in(root.path())
            .arg("install")
            .assert()
            .success();
        python(root.path()).args(["-c", "import alpha, helper.origin; import importlib.metadata as m; assert alpha.VERSION == '1.0'; assert helper.origin.SOURCE == 'index'; assert m.distribution('helper').read_text('direct_url.json') is None"]).assert().success();
        let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        assert!(!lock.contains(&url), "{lock}");
        pacquet_in(root.path())
            .args(["install", "--offline", "--frozen-lockfile"])
            .assert()
            .success();
        artifact.assert_async().await;
    }
}

#[tokio::test]
async fn rejected_extras_cannot_leave_direct_source_provenance_on_index_wheels() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let url = format!("{}/helper-1.0-py3-none-any.whl", server.url());
    let artifact = server
        .mock("GET", "/helper-1.0-py3-none-any.whl")
        .with_body(wheel("helper", "1.0", "Requires-Dist: conflict>=2\n", &[]))
        .expect(1)
        .create_async()
        .await;
    let _alpha = serve(&mut server, "alpha", &[
        ("2.0", wheel("alpha", "2.0", &format!("Provides-Extra: helpers\nRequires-Dist: helper @ {url} ; extra == 'helpers'\n"), &[])),
        ("1.0", wheel("alpha", "1.0", "Provides-Extra: helpers\nRequires-Dist: helper>=1 ; extra == 'helpers'\n", &[])),
    ]).await;
    let _helper = serve(&mut server, "helper", &[("1.0", wheel("helper", "1.0", "", &[]))]).await;
    let _conflict = serve(
        &mut server,
        "conflict",
        &[("1.0", wheel("conflict", "1.0", "", &[])), ("2.0", wheel("conflict", "2.0", "", &[]))],
    )
    .await;
    project(root.path(), &server.url(), &["alpha[helpers]", "conflict<2"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path()).args(["-c", "import alpha, helper; import importlib.metadata as m; assert alpha.VERSION == '1.0'; assert m.distribution('helper').read_text('direct_url.json') is None"]).assert().success();
    artifact.assert_async().await;
}

#[tokio::test]
async fn unhashed_public_http_wheels_are_rejected_before_downloading() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(
        root.path(),
        &server.url(),
        &["alpha @ http://example.test/alpha-1.0-py3-none-any.whl"],
    );
    super::super::assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "public HTTP require a sha256 hash",
    );
}

#[tokio::test]
async fn requirements_files_accept_direct_wheel_requirements() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let url = format!("{}/alpha-1.0-py3-none-any.whl", server.url());
    let artifact = server
        .mock("GET", "/alpha-1.0-py3-none-any.whl")
        .with_body(wheel("alpha", "1.0", "", &[]))
        .expect(1)
        .create_async()
        .await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    fs::write(root.path().join("requirements.txt"), format!("alpha @ {url}\n")).unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path()).args(["-c", "import alpha; import importlib.metadata as m, json; assert json.loads(m.distribution('alpha').read_text('direct_url.json'))['url'].endswith('/alpha-1.0-py3-none-any.whl')"]).assert().success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(
        !root
            .path()
            .join("pyproject.toml")
            .exists(),
    );
    artifact.assert_async().await;
}
