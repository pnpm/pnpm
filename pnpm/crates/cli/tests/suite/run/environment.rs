#[cfg(unix)]
use super::write_executable;
use super::{CommandExtra, CommandTempCwd, fs, json};
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};

/// The same local-prefix resolution as
/// [pnpm/pnpm#14622](https://github.com/pnpm/pnpm/issues/14622), which
/// `pnpm bin` reported.
#[cfg(unix)]
#[test]
fn run_from_a_plain_subdir_runs_the_projects_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    // A relative marker, so where the file lands pins the working
    // directory the script ran in.
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "touch-marker": "touch marker.txt" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    let subdir = workspace.join("src/utils");
    fs::create_dir_all(&subdir).expect("create the subdirectory");

    pacquet.with_current_dir(&subdir).with_args(["run", "touch-marker"]).assert().success();
    assert!(workspace.join("marker.txt").exists(), "the script should have run in the project");
    assert!(!subdir.join("marker.txt").exists(), "the script should not have run in the subdir");

    drop(root);
}

/// Regression test for
/// [pnpm/pnpm#14664](https://github.com/pnpm/pnpm/issues/14664): a
/// `Cargo.toml` or a `pyproject.toml` bounds the prefix walk for the
/// commands that install dependencies, but such a directory holds no
/// `package.json#scripts`, so `run` walks past it to the npm project
/// around it.
#[cfg(unix)]
#[test]
fn run_from_an_ecosystem_subdir_runs_the_npm_projects_script() {
    for (manifest_name, contents) in [
        ("Cargo.toml", "[package]\nname = \"member\"\nversion = \"0.1.0\"\n"),
        ("pyproject.toml", "[project]\nname = 'member'\nversion = '1.0'\n"),
    ] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let manifest = json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": { "touch-marker": "touch marker.txt" },
        })
        .to_string();
        fs::write(workspace.join("package.json"), manifest).expect("write package.json");
        let member = workspace.join("member");
        fs::create_dir(&member).expect("create the member dir");
        fs::write(member.join(manifest_name), contents).expect("write the ecosystem manifest");

        pacquet.with_current_dir(&member).with_args(["run", "touch-marker"]).assert().success();
        assert!(
            workspace.join("marker.txt").exists(),
            "the script should have run in the npm project, not the {manifest_name} member",
        );

        drop(root);
    }
}

#[test]
fn run_preserves_parent_tmpdir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let alternate_tmpdir = workspace.join("project-tmp");
    fs::create_dir(&alternate_tmpdir).expect("create alternate temp dir");
    fs::write(
        workspace.join("show-tmp.js"),
        "require('fs').writeFileSync('tmpdir.json', JSON.stringify({ \
env: process.env.TMPDIR, os: require('os').tmpdir() }))",
    )
    .expect("write show-tmp.js");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": { "show-tmp": "node show-tmp.js" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_env("TMPDIR", &alternate_tmpdir)
        .with_arg("run")
        .with_arg("show-tmp")
        .assert()
        .success();

    let recorded: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("tmpdir.json")).expect("read tmpdir.json"),
    )
    .expect("parse tmpdir.json");
    let expected_tmpdir = alternate_tmpdir.to_string_lossy();
    assert_eq!(recorded["env"], expected_tmpdir.as_ref());
    if cfg!(not(windows)) {
        assert_eq!(recorded["os"], expected_tmpdir.as_ref());
    }

    drop(root);
}

/// A script that invokes a locally-installed binary resolves it through
/// `node_modules/.bin`, which `pnpm run` prepends to `PATH`.
#[cfg(unix)]
#[test]
fn run_finds_local_bin_on_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("marker.txt");
    write_executable(
        &bin_dir.join("say-hi"),
        &format!("#!/bin/sh\ntouch \"{}\"\n", marker.display()),
    );
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "hi": "say-hi" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("run").with_arg("hi").assert().success();
    assert!(marker.exists(), "the local bin should be resolved via node_modules/.bin");

    drop(root);
}

/// Running a script from a workspace member resolves binaries from the
/// workspace root's `node_modules/.bin` — pnpm puts it on PATH via
/// `extraBinPaths`, so root-level dev tools are callable from every
/// workspace project.
#[cfg(unix)]
#[test]
fn run_finds_workspace_root_bin_on_path() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - project\n")
        .expect("write pnpm-workspace.yaml");
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create workspace-root node_modules/.bin");
    write_executable(&bin_dir.join("root-tool"), "#!/bin/sh\ntouch root-tool-ran.txt\n");
    let project = workspace.join("project");
    fs::create_dir_all(&project).expect("create project dir");
    let manifest = json!({
        "name": "project",
        "version": "0.0.0",
        "scripts": { "build": "root-tool" },
    })
    .to_string();
    fs::write(project.join("package.json"), manifest).expect("write package.json");

    std::process::Command::cargo_bin("pnpm")
        .expect("find pacquet binary")
        .with_current_dir(&project)
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();
    assert!(
        project.join("root-tool-ran.txt").exists(),
        "the workspace root's node_modules/.bin should be on the script's PATH",
    );

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_runs_script_before_local_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("source.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf bin > \"{}\"\n", marker.display()),
    );
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "commitlint": format!(r#"printf script > "{}""#, marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("commitlint").assert().success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "script");

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_runs_local_bin_when_script_is_missing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("args.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", marker.display()),
    );
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {},
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet
        .with_args(["commitlint", "--edit", "--config=commitlint.config.cjs"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&marker).expect("read marker"),
        "--edit\n--config=commitlint.config.cjs\n",
    );

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_runs_local_bin_without_package_json() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("args.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", marker.display()),
    );

    pacquet.with_args(["commitlint", "--edit", "COMMIT_EDITMSG"]).assert().success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "--edit\nCOMMIT_EDITMSG\n");

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_forwards_dotted_config_args_to_local_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("args.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", marker.display()),
    );

    pacquet.with_args(["commitlint", "--config.foo=bar"]).assert().success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "--config.foo=bar\n");

    drop(root);
}

/// With `preferSymlinkedExecutables`, symlinked bins have no shim to
/// carry a `NODE_PATH` block, so the config exports one pointing at
/// the virtual store's hidden `node_modules` — pnpm's
/// `pnpm run with preferSymlinkedExecutables true` test.
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn run_exports_node_path_when_prefer_symlinked_executables() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker_path = workspace.join("node-path.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "build": format!(r#"sh -c 'printf %s "$NODE_PATH" > "{}"'"#, marker_path.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "preferSymlinkedExecutables: true\n")
        .expect("write pnpm-workspace.yaml");

    pacquet.with_args(["run", "build"]).assert().success();
    let node_path = fs::read_to_string(&marker_path).expect("read marker");
    assert!(
        node_path.contains("node_modules/.pnpm/node_modules"),
        "NODE_PATH must point at the virtual store's hidden node_modules: {node_path:?}",
    );

    drop(root);
}

/// An explicit `virtualStoreDir` redirects the exported `NODE_PATH` —
/// pnpm's `pnpm run with preferSymlinkedExecutables and custom
/// virtualStoreDir` test.
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn run_exports_node_path_from_a_custom_virtual_store_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker_path = workspace.join("node-path.txt");
    let virtual_store_dir = workspace.join("foo/bar");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "build": format!(r#"sh -c 'printf %s "$NODE_PATH" > "{}"'"#, marker_path.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "virtualStoreDir: {}\npreferSymlinkedExecutables: true\n",
            virtual_store_dir.display(),
        ),
    )
    .expect("write pnpm-workspace.yaml");

    pacquet.with_args(["run", "build"]).assert().success();
    let node_path = fs::read_to_string(&marker_path).expect("read marker");
    let expected = virtual_store_dir.join("node_modules");
    assert!(
        node_path.contains(&expected.display().to_string()),
        "NODE_PATH must point inside the custom virtual store: {node_path:?}",
    );

    drop(root);
}
