use super::{
    add_python_settings, assert_failure_contains, pacquet_in, project, python, serve, wheel,
};
use assert_cmd::assert::OutputAssertExt;
use std::fs;

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
        "ERR_PNPM_UNSUPPORTED_PYTHON_RULE",
    );
}

#[tokio::test]
async fn overrides_replace_extras_and_keep_new_transitive_direct_sources_active() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let url = format!("{}/helper-1.0-py3-none-any.whl", server.url());
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta[old]<2\n", &[]))],
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
                    "Provides-Extra: helpers\nProvides-Extra: old\nRequires-Dist: obsolete ; extra == 'old'\nRequires-Dist: helper @ {url} ; extra == 'helpers'\n",
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
    let obsolete = server
        .mock("GET", "/simple/obsolete/")
        .expect(0)
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
    obsolete.assert_async().await;
}
