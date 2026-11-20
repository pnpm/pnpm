use super::{
    add_python_settings,
    pacquet_in,
    project,
    python,
    serve,
    wheel,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::fs;

#[tokio::test]
async fn update_preserves_optional_dependencies_table_and_updates_extras() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0.0", wheel("alpha", "1.0.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0.0", wheel("beta", "1.0.0", "", &[]))]).await;
    let _gamma = serve(&mut server, "gamma", &[("1.0.0", wheel("gamma", "1.0.0", "", &[]))]).await;

    project(root.path(), &server.url(), &[]);
    add_python_settings(root.path(), "  extras: [cli]\n");
    let initial_manifest = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
requires-python = \">=3.10\"
dependencies = [\"beta>=1.0.0,<2.0.0\"]

# Keep section comments and unselected extras intact.
[project.optional-dependencies]
cli = [\"alpha>=1.0.0,<2.0.0\"]
docs = ['gamma>=1.0.0,<2.0.0']
";
    fs::write(root.path().join("pyproject.toml"), initial_manifest).unwrap();

    pacquet_in(root.path())
        .with_arg("install")
        .assert()
        .success();

    let _alpha = serve(
        &mut server,
        "alpha",
        &[
            ("1.0.0", wheel("alpha", "1.0.0", "", &[])),
            ("1.5.0", wheel("alpha", "1.5.0", "", &[])),
            ("2.0.0", wheel("alpha", "2.0.0", "", &[])),
        ],
    )
    .await;
    let _beta = serve(
        &mut server,
        "beta",
        &[
            ("1.0.0", wheel("beta", "1.0.0", "", &[])),
            ("1.5.0", wheel("beta", "1.5.0", "", &[])),
            ("2.0.0", wheel("beta", "2.0.0", "", &[])),
        ],
    )
    .await;

    // Updating only a main dependency temporarily stages unselected extras and restores them.
    pacquet_in(root.path())
        .with_args(["update", "pypi:beta"])
        .assert()
        .success();
    let after_main_update = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
    let expected_after_main = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
requires-python = \">=3.10\"
dependencies = [\"beta>=1.5.0,<2.0.0\"]

# Keep section comments and unselected extras intact.
[project.optional-dependencies]
cli = [\"alpha>=1.0.0,<2.0.0\"]
docs = ['gamma>=1.0.0,<2.0.0']
";
    eprintln!("AFTER MAIN UPDATE:\n{after_main_update}\n");
    assert_eq!(after_main_update, expected_after_main);
    python(root.path())
        .with_args([
            "-c",
            "import alpha, beta; assert alpha.VERSION == '1.0.0' and beta.VERSION == '1.5.0'",
        ])
        .assert()
        .success();

    pacquet_in(root.path())
        .with_arg("update")
        .assert()
        .success();
    let after_full_update = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
    let expected_after_full = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
requires-python = \">=3.10\"
dependencies = [\"beta>=1.5.0,<2.0.0\"]

# Keep section comments and unselected extras intact.
[project.optional-dependencies]
cli = [\"alpha>=1.5.0,<2.0.0\"]
docs = ['gamma>=1.0.0,<2.0.0']
";
    eprintln!("AFTER FULL UPDATE:\n{after_full_update}\n");
    assert_eq!(after_full_update, expected_after_full);
    python(root.path())
        .with_args([
            "-c",
            "import alpha, beta; assert alpha.VERSION == '1.5.0' and beta.VERSION == '1.5.0'",
        ])
        .assert()
        .success();
}

#[tokio::test]
async fn update_preserves_inline_optional_dependencies_table() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0.0", wheel("alpha", "1.0.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0.0", wheel("beta", "1.0.0", "", &[]))]).await;

    project(root.path(), &server.url(), &[]);
    add_python_settings(root.path(), "  extras: [cli]\n");
    let inline_manifest = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
requires-python = \">=3.10\"
dependencies = [\"beta>=1.0.0\"]
optional-dependencies = { cli = [\"alpha==1.0.0\"] }
";
    fs::write(root.path().join("pyproject.toml"), inline_manifest).unwrap();

    pacquet_in(root.path())
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0.0", wheel("alpha", "1.0.0", "", &[])), ("2.0.0", wheel("alpha", "2.0.0", "", &[]))],
    )
    .await;

    pacquet_in(root.path())
        .with_args(["update", "--latest", "pypi:alpha", "--lockfile-only"])
        .assert()
        .success();
    let updated = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
    let expected = "\
[project]
name = \"my-app\"
version = \"0.1.0\"
requires-python = \">=3.10\"
dependencies = [\"beta>=1.0.0\"]
optional-dependencies = { cli = [\"alpha==2.0.0\"] }
";
    eprintln!("UPDATED INLINE:\n{updated}\n");
    assert_eq!(updated, expected);
}
