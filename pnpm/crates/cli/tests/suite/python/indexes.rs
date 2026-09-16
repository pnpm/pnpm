use super::{
    add_python_settings, assert_failure_contains, pacquet_in, project, python, serve,
    serve_with_index_auth, wheel,
};
use assert_cmd::assert::OutputAssertExt;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::fs;

#[tokio::test]
async fn extra_indexes_have_priority_and_cache_missing_packages_for_offline_resolution() {
    let root = tempfile::tempdir().unwrap();
    let mut primary = mockito::Server::new_async().await;
    let mut extra = mockito::Server::new_async().await;
    let _primary_alpha =
        serve(&mut primary, "alpha", &[("9.0", wheel("alpha", "9.0", "", &[]))]).await;
    let authorization = format!("Basic {}", STANDARD.encode("extra-user:extra-secret"));
    let _extra_alpha = serve_with_index_auth(
        &mut extra,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta\n", &[]))],
        Some(&authorization),
    )
    .await;
    let _primary_beta =
        serve(&mut primary, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    let missing = extra
        .mock("GET", "/simple/beta/")
        .match_header("authorization", authorization.as_str())
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    project(root.path(), &primary.url(), &["alpha"]);
    let mut extra_url: url::Url = extra.url().parse().unwrap();
    extra_url.set_username("extra-user").unwrap();
    extra_url
        .set_password(Some("extra-secret"))
        .unwrap();
    add_python_settings(
        root.path(),
        &format!("  extraIndexUrls: ['{}/simple/']\n", extra_url.as_str().trim_end_matches('/')),
    );
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, beta; assert alpha.VERSION == '1.0'"])
        .assert()
        .success();
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("{lock}");
    assert!(lock.contains("extra-indexes"));
    assert!(!lock.contains("extra-secret"));
    fs::remove_file(root.path().join("pylock.toml")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline"])
        .assert()
        .success();
    missing.assert_async().await;
    add_python_settings(root.path(), "  constraints: ['alpha>=2']\n");
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--frozen-lockfile"]),
        "overrides or constraints changed",
    );
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "Python dependency resolution failed",
    );
}

#[tokio::test]
async fn dependency_overrides_and_constraints_apply_during_install_and_frozen_replay() {
    for from_uv in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _alpha = serve(
            &mut server,
            "alpha",
            &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta<2\n", &[]))],
        )
        .await;
        let _beta = serve(
            &mut server,
            "beta",
            &[
                ("1.0", wheel("beta", "1.0", "", &[])),
                ("2.0", wheel("beta", "2.0", "", &[])),
                ("3.0", wheel("beta", "3.0", "", &[])),
            ],
        )
        .await;
        project(root.path(), &server.url(), &["alpha"]);
        if from_uv {
            let path = root.path().join("pyproject.toml");
            let manifest = fs::read_to_string(&path).unwrap();
            fs::write(path, format!("{manifest}\n[tool.uv]\noverride-dependencies = ['beta>=2']\nconstraint-dependencies = ['beta<3', 'absent==1']\n")).unwrap();
        } else {
            add_python_settings(
                root.path(),
                "  overrides: ['beta>=2']\n  constraints: ['beta<3', 'absent==1']\n",
            );
        }
        pacquet_in(root.path())
            .arg("install")
            .assert()
            .success();
        python(root.path())
            .args(["-c", "import beta; assert beta.VERSION == '2.0'"])
            .assert()
            .success();
        pacquet_in(root.path())
            .args(["install", "--frozen-lockfile", "--offline"])
            .assert()
            .success();
    }
}

#[tokio::test]
async fn extra_index_errors_do_not_fall_back_to_another_index() {
    let root = tempfile::tempdir().unwrap();
    let mut primary = mockito::Server::new_async().await;
    let mut extra = mockito::Server::new_async().await;
    let unused = primary
        .mock("GET", "/simple/alpha/")
        .expect(0)
        .create_async()
        .await;
    let denied = extra
        .mock("GET", "/simple/alpha/")
        .with_status(403)
        .create_async()
        .await;
    project(root.path(), &primary.url(), &["alpha"]);
    add_python_settings(root.path(), &format!("  extraIndexUrls: ['{}/simple/']\n", extra.url()));
    assert_failure_contains(pacquet_in(root.path()).arg("install"), "403 Forbidden");
    unused.assert_async().await;
    denied.assert_async().await;
}

#[tokio::test]
async fn uv_workspace_root_rules_apply_to_members_without_a_root_project() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta<2\n", &[]))],
    )
    .await;
    let _beta = serve(
        &mut server,
        "beta",
        &[("2.0", wheel("beta", "2.0", "", &[])), ("3.0", wheel("beta", "3.0", "", &[]))],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    fs::write(root.path().join("pyproject.toml"), "[tool.uv]\noverride-dependencies = ['beta>=2']\nconstraint-dependencies = ['beta<3']\n[tool.uv.workspace]\nmembers = ['packages/*']\n").unwrap();
    let member = root.path().join("packages/app");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\ndependencies = ['alpha']\n[tool.uv]\noverride-dependencies = ['beta>=3']\n").unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(&member)
        .args(["-c", "import beta; assert beta.VERSION == '2.0'"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--frozen-lockfile", "--offline"])
        .assert()
        .success();
    fs::write(root.path().join("pyproject.toml"), "[tool.uv]\noverride-dependencies = ['beta>=2']\nconstraint-dependencies = ['beta<2']\n[tool.uv.workspace]\nmembers = ['packages/*']\n").unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--frozen-lockfile"]),
        "overrides or constraints changed",
    );
}

#[tokio::test]
async fn version_constraints_apply_to_direct_wheels_without_changing_their_source() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let artifact = server
        .mock("GET", "/alpha-1.0-py3-none-any.whl")
        .with_body(wheel("alpha", "1.0", "", &[]))
        .create_async()
        .await;
    let url = format!("{}/alpha-1.0-py3-none-any.whl", server.url());
    project(root.path(), &server.url(), &[&format!("alpha @ {url}")]);
    add_python_settings(root.path(), "  constraints: ['alpha<2']\n");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    artifact.assert_async().await;
    let settings = root.path().join("pnpm-workspace.yaml");
    let contents = fs::read_to_string(&settings).unwrap();
    fs::write(settings, contents.replace("alpha<2", "alpha>=2")).unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "Python dependency resolution failed",
    );
}

#[tokio::test]
async fn dependency_rules_reject_url_requirements() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "https://example.test", &[]);
    add_python_settings(
        root.path(),
        "  overrides: ['alpha @ https://example.test/alpha-1.0-py3-none-any.whl']\n",
    );
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "overrides and constraints must use registry version requirements",
    );
}

#[tokio::test]
async fn override_extras_keep_their_transitive_direct_sources_active() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let url = format!("{}/helper-1.0-py3-none-any.whl", server.url());
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta<2\n", &[]))],
    )
    .await;
    let _beta = serve(
        &mut server,
        "beta",
        &[(
            "2.0",
            wheel(
                "beta",
                "2.0",
                &format!(
                    "Provides-Extra: helpers\nRequires-Dist: helper @ {url} ; extra == 'helpers'\n",
                ),
                &[],
            ),
        )],
    )
    .await;
    let artifact = server
        .mock("GET", "/helper-1.0-py3-none-any.whl")
        .with_body(wheel("helper", "1.0", "", &[]))
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha"]);
    add_python_settings(root.path(), "  overrides: ['beta[helpers]>=2']\n");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, beta, helper"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    artifact.assert_async().await;
}
