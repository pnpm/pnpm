use super::{TINY_BACKEND, project, python, python_project, serve, serve_backends, wheel};
use assert_cmd::prelude::*;
use std::fs;

#[tokio::test]
async fn requirements_only_projects_install_and_invalidate_the_frozen_lockfile() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    fs::write(
        root.path().join("requirements.txt"),
        "# dependencies\n-r base.txt\nalpha \\\n ==1.0 # pinned\n",
    )
    .unwrap();
    fs::write(root.path().join("base.txt"), "beta==1.0\n").unwrap();
    super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, beta"])
        .assert()
        .success();
    super::pacquet_in(root.path())
        .args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    fs::write(root.path().join("base.txt"), "beta>=1.0\n").unwrap();
    super::pacquet_in(root.path())
        .args(["install", "--frozen-lockfile"])
        .assert()
        .failure();
    assert!(
        !root
            .path()
            .join("pyproject.toml")
            .exists(),
    );
}

#[tokio::test]
async fn requirements_alongside_a_tool_only_manifest_install() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::write(root.path().join("pyproject.toml"), "[tool.ruff]\nline-length = 100\n").unwrap();
    fs::write(root.path().join("requirements.txt"), "alpha==1.0\n").unwrap();
    super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
}

#[test]
fn unsupported_requirements_directives_and_cycles_fail_with_context() {
    for contents in
        ["--index-url https://example.com\n", "-r requirements.txt\n", "alpha --hash=sha256:abc\n"]
    {
        let root = tempfile::tempdir().unwrap();
        project(root.path(), "http://127.0.0.1:1", &[]);
        fs::remove_file(root.path().join("pyproject.toml")).unwrap();
        fs::write(root.path().join("requirements.txt"), contents).unwrap();
        let output = super::pacquet_in(root.path())
            .arg("install")
            .assert()
            .failure()
            .get_output()
            .clone();
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("requirements.txt"), "{error}");
    }
}

#[tokio::test]
async fn dynamic_dependencies_use_the_metadata_hook_or_wheel_fallback() {
    for hook in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let backend = TINY_BACKEND.replace(
            r#"project = manifest["project"]"#,
            r#"project = dict(manifest["project"])"#,
        );
        let backend = backend.replace(r#"project = dict(manifest["project"])"#, "project = dict(manifest[\"project\"])\n    project[\"dependencies\"] = open(\"requirements.txt\").read().splitlines()");
        let backend = if hook {
            format!(
                "{backend}\n\ndef prepare_metadata_for_build_wheel(directory, config_settings=None):\n    import pathlib\n    output = pathlib.Path(directory) / 'app-1.0.dist-info'\n    output.mkdir()\n    (output / 'METADATA').write_text('Metadata-Version: 2.4\\nName: app\\nVersion: 1.0\\nRequires-Python: >=3.10\\nRequires-Dist: ' + open('requirements.txt').read().strip() + '\\n')\n    return output.name\n",
            )
        } else {
            backend
        };
        let _backend = serve(
            &mut server,
            "tinybackend",
            &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
        )
        .await;
        let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
        project(root.path(), &server.url(), &[]);
        python_project(root.path(), "app", "dynamic = ['dependencies']");
        fs::write(root.path().join("requirements.txt"), "alpha==1.0\n").unwrap();
        super::pacquet_in(root.path())
            .args(["install", "--lockfile-only"])
            .assert()
            .success();
        assert!(!root.path().join(".venv").exists());
        super::pacquet_in(root.path())
            .args(["install", "--frozen-lockfile"])
            .assert()
            .success();
        python(root.path())
            .args(["-c", "import app, alpha"])
            .assert()
            .success();
        fs::write(root.path().join("requirements.txt"), "alpha>=1.0\n").unwrap();
        super::pacquet_in(root.path())
            .args(["install", "--frozen-lockfile"])
            .assert()
            .failure();
    }
}

#[tokio::test]
async fn dynamic_metadata_never_runs_an_unapproved_backend() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    python_project(root.path(), "app", "dynamic = ['dependencies']");
    let config = root.path().join("pnpm-workspace.yaml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .unwrap()
            .replace("pkg:pypi/tinybackend: true", "pkg:pypi/tinybackend: false"),
    )
    .unwrap();
    let output = super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .failure()
        .get_output()
        .clone();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        super::flatten_report(&error)
            .contains(&super::flatten_report("approved build requirements")),
        "{error}",
    );
    assert!(!root.path().join("pylock.toml").exists());
}

#[tokio::test]
async fn dynamic_dependencies_and_versions_resolve_workspace_sources() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let backend = TINY_BACKEND.replace(
        r#"project = manifest["project"]"#,
        "project = dict(manifest[\"project\"])\n    if project[\"name\"] == \"app\":\n        project[\"dependencies\"] = [\"mylib==0.0.1\"]",
    );
    let backend = format!(
        r"{backend}

def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    assert metadata_directory is None
    return _build(wheel_directory, False)
",
    );
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    let library = root.path().join("packages/mylib");
    python_project(&library, "mylib", "dependencies = []");
    fs::write(
        library.join("pyproject.toml"),
        fs::read_to_string(library.join("pyproject.toml"))
            .unwrap()
            .replace("version = '1.0'", "dynamic = ['version']"),
    )
    .unwrap();
    let app = root.path().join("packages/app");
    python_project(
        &app,
        "app",
        "dynamic = ['dependencies']\n[tool.uv.sources]\nmylib = {workspace = true, editable = false}",
    );
    super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(&app)
        .args(["-c", "import mylib; assert mylib.MARKER == 'workspace mylib'"])
        .assert()
        .success();
}

#[tokio::test]
async fn a_noneditable_dynamic_source_receives_its_prepared_metadata_directory() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let backend = format!(
        r#"{TINY_BACKEND}

def prepare_metadata_for_build_wheel(directory, config_settings=None):
    import pathlib
    project = tomllib.load(open("pyproject.toml", "rb"))["project"]
    output = pathlib.Path(directory) / (project["name"] + "-0.0.1.dist-info")
    output.mkdir()
    (output / "METADATA").write_text("Metadata-Version: 2.4\nName: " + project["name"] + "\nVersion: 0.0.1\nRequires-Python: >=3.10\n")
    (output / "sentinel").write_text("prepared")
    return output.name

def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    import pathlib
    assert (pathlib.Path(metadata_directory) / "sentinel").read_text() == "prepared"
    return _build(wheel_directory, False)
"#,
    );
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
    )
    .await;
    project(root.path(), &server.url(), &["mylib==0.0.1"]);
    let manifest = root.path().join("pyproject.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[tool.uv.sources]\nmylib = {{path = 'mylib', editable = false}}\n",
            fs::read_to_string(&manifest).unwrap(),
        ),
    )
    .unwrap();
    let library = root.path().join("mylib");
    python_project(&library, "mylib", "dependencies = []");
    fs::write(
        library.join("pyproject.toml"),
        fs::read_to_string(library.join("pyproject.toml"))
            .unwrap()
            .replace("version = '1.0'", "dynamic = ['version']"),
    )
    .unwrap();
    super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import mylib"])
        .assert()
        .success();
}

#[tokio::test]
async fn dynamic_metadata_cannot_omit_or_change_static_dependencies() {
    for declaration in ["dynamic = ['version']", "version = '1.0'\ndynamic = ['requires-python']"] {
        for dependencies in ["[]", r#"["alpha>=2"]"#] {
            let root = tempfile::tempdir().unwrap();
            let mut server = mockito::Server::new_async().await;
            let backend = TINY_BACKEND.replace(
                r#"project = manifest["project"]"#,
                &format!(
                    r#"project = dict(manifest["project"])
    project["dependencies"] = {dependencies}"#,
                ),
            );
            let _backend = serve(
                &mut server,
                "tinybackend",
                &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
            )
            .await;
            project(root.path(), &server.url(), &[]);
            python_project(root.path(), "app", "dependencies = ['alpha']");
            let manifest = root.path().join("pyproject.toml");
            fs::write(
                &manifest,
                fs::read_to_string(&manifest).unwrap().replace("version = '1.0'", declaration),
            )
            .unwrap();
            super::assert_failure_contains(
                super::pacquet_in(root.path()).arg("install"),
                "omits static project dependencies",
            );
            assert!(!root.path().join("pylock.toml").exists());
        }
    }
}

#[tokio::test]
async fn the_final_wheel_must_preserve_prepared_python_support_and_extras() {
    for field in ["Requires-Python: >=3.11", "Provides-Extra: cli"] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let backend = TINY_BACKEND.replace(
            r#"entries[dist_info + "/METADATA"] = metadata"#,
            &format!(r#"entries[dist_info + "/METADATA"] = metadata.replace("Requires-Python: >=3.10\n", "") + {field:?} + '\n'"#),
        );
        let backend = format!(
            r#"{backend}

def prepare_metadata_for_build_wheel(directory, config_settings=None):
    import pathlib
    output = pathlib.Path(directory) / "app-1.0.dist-info"
    output.mkdir()
    (output / "METADATA").write_text("Metadata-Version: 2.4\nName: app\nVersion: 1.0\nRequires-Python: >=3.10\n")
    return output.name
"#,
        );
        let _backend = serve(
            &mut server,
            "tinybackend",
            &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
        )
        .await;
        project(root.path(), &server.url(), &[]);
        python_project(root.path(), "app", "dynamic = ['dependencies']");
        super::assert_failure_contains(
            super::pacquet_in(root.path()).arg("install"),
            "differs from its prepared metadata",
        );
        assert!(!root.path().join("pylock.toml").exists());
    }
}

#[tokio::test]
async fn inactive_dynamic_dependencies_do_not_shadow_workspace_projects() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let backend = TINY_BACKEND.replace(r#"project = manifest["project"]"#, "project = dict(manifest[\"project\"])\n    if project[\"name\"] == \"app\":\n        project[\"dependencies\"] = [\"mylib; python_version < '2'\", \"mylib; extra == 'cli'\"]");
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    python_project(&root.path().join("packages/mylib"), "mylib", "dependencies = []");
    python_project(&root.path().join("packages/app"), "app", "dynamic = ['dependencies']");
    super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
}

#[tokio::test]
async fn a_dependency_extra_selects_dynamic_workspace_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let backend = TINY_BACKEND.replace(r#"project = manifest["project"]"#, "project = dict(manifest[\"project\"])\n    if project[\"name\"] == \"mylib\":\n        project[\"dependencies\"] = [\"beta; extra == 'foo'\"]");
    let backend = backend.replace(r#"entries[dist_info + "/METADATA"] = metadata"#, "if name == 'mylib':\n        metadata += 'Provides-Extra: foo\\n'\n    entries[dist_info + \"/METADATA\"] = metadata");
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    python_project(&root.path().join("packages/beta"), "beta", "dependencies = []");
    python_project(
        &root.path().join("packages/mylib"),
        "mylib",
        "dependencies = []\ndynamic = ['optional-dependencies']\n[tool.uv.sources]\nbeta = {workspace = true}",
    );
    python_project(
        &root.path().join("packages/app"),
        "app",
        "dependencies = ['mylib[foo]']\n[tool.uv.sources]\nmylib = {workspace = true}",
    );
    super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(&root.path().join("packages/app"))
        .args(["-c", "import beta; assert beta.MARKER == 'workspace beta'"])
        .assert()
        .success();
}

#[tokio::test]
async fn dynamic_dependencies_cannot_change_static_python_support() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let backend = TINY_BACKEND.replace(r#"if "requires-python" in project:"#, "if False:");
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    python_project(root.path(), "app", "dynamic = ['dependencies']");
    super::assert_failure_contains(
        super::pacquet_in(root.path()).args(["install", "--lockfile-only"]),
        "differs from static project requires-python",
    );
    assert!(!root.path().join("pylock.toml").exists());
}

#[tokio::test]
async fn dynamic_versions_cannot_omit_rename_or_add_static_extras() {
    for extras in ["", "Provides-Extra: other\n", "Provides-Extra: cli\nProvides-Extra: other\n"] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let backend = TINY_BACKEND.replace(
            r#"entries[dist_info + "/METADATA"] = metadata"#,
            &format!(
                r#"metadata += "Requires-Dist: alpha; extra == 'cli'\n" + {extras:?}
    entries[dist_info + "/METADATA"] = metadata"#,
            ),
        );
        let _backend = serve(
            &mut server,
            "tinybackend",
            &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
        )
        .await;
        project(root.path(), &server.url(), &[]);
        python_project(root.path(), "app", "[project.optional-dependencies]\ncli = ['alpha']");
        let path = root.path().join("pyproject.toml");
        fs::write(
            &path,
            fs::read_to_string(&path).unwrap().replace("version = '1.0'", "dynamic = ['version']"),
        )
        .unwrap();
        super::assert_failure_contains(
            super::pacquet_in(root.path()).args(["install", "--lockfile-only"]),
            "differs from static project extras",
        );
        assert!(!root.path().join("pylock.toml").exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn dynamic_python_support_reprepares_metadata_with_the_selected_interpreter() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let shims = outside.path().join("interpreters");
    super::interpreter_shim(&shims, "python3.11", "3.11.9");
    super::interpreter_shim(&shims, "python3.12", "3.12.7");
    let mut server = mockito::Server::new_async().await;
    let backend = format!(
        r#"{TINY_BACKEND}

def prepare_metadata_for_build_wheel(directory, config_settings=None):
    import pathlib
    count = pathlib.Path("metadata-count.txt")
    count.write_text(str(int(count.read_text()) + 1 if count.exists() else 1))
    output = pathlib.Path(directory) / "app-1.0.dist-info"
    output.mkdir()
    (output / "METADATA").write_text("Metadata-Version: 2.4\nName: app\nVersion: 1.0\nRequires-Python: ==3.12.7\n")
    return output.name
"#,
    );
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", &backend)]))],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    python_project(root.path(), "app", "dynamic = ['requires-python']");
    let path = root.path().join("pyproject.toml");
    fs::write(
        &path,
        fs::read_to_string(&path).unwrap().replace("requires-python = '>=3.10'\n", ""),
    )
    .unwrap();
    fs::write(root.path().join(".python-version"), "3.11\n").unwrap();
    super::pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .env("PATH", format!("{}:{}", shims.display(), std::env::var("PATH").unwrap()))
        .assert()
        .success();
    assert_eq!(super::selected_python(root.path()), "3.12.7");
    assert_eq!(fs::read_to_string(root.path().join("metadata-count.txt")).unwrap(), "2");
}
