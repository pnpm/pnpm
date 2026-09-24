use super::{
    CommandExtra, CommandTempCwd, GitRepoFixture, Value, allow_builds, append_workspace_yaml_key,
    assert_eq, assert_success, fs, importer_version, json, pnpm_at, read_lockfile,
    write_dependencies,
};
use assert_cmd::assert::OutputAssertExt;

/// TS: `run prepare script for git-hosted dependencies`
/// (`lifecycleScripts.ts:311`).
///
/// A git dependency has no published tarball, so pnpm builds it on the
/// way in: the install lifecycle runs once for the checkout, `prepare`
/// packs it, and the lifecycle runs again for the installed package.
#[test]
fn run_prepare_script_for_git_hosted_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "test-git-fetch");
    repo.write_file(
        "append.js",
        "const fs = require('fs')\n\
         const file = 'output.json'\n\
         let scripts = []\n\
         try { scripts = JSON.parse(fs.readFileSync(file, 'utf8')) } catch {}\n\
         scripts.push(process.argv[2])\n\
         fs.writeFileSync(file, JSON.stringify(scripts))\n",
    );
    repo.write_file("index.js", "module.exports = 'ok'\n");
    repo.write_file(
        "package.json",
        r#"{"name":"test-git-fetch","version":"1.0.0","main":"index.js","scripts":{"prepare":"node append prepare","preinstall":"node append preinstall","install":"node append install","postinstall":"node append postinstall"}}"#,
    );
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);

    write_dependencies(&workspace, &[("test-git-fetch", &spec)]);
    allow_builds(&workspace, &[&format!("test-git-fetch@{spec}")]);

    pacquet
        .with_args(["install"])
        .assert()
        .success();

    let output: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace.join("node_modules/test-git-fetch/output.json"))
            .expect("read the script log the package wrote"),
    )
    .expect("parse the script log");
    assert_eq!(
        output,
        json!([
            "preinstall",
            "install",
            "postinstall",
            "prepare",
            "preinstall",
            "install",
            "postinstall",
        ]),
    );

    drop((root, npmrc_info));
}

#[test]
fn prepared_git_package_in_shared_store_still_requires_project_approval() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "shared-prepare");
    repo.write_file(
        "package.json",
        r#"{"name":"shared-prepare","version":"1.0.0","files":["package.json","prepare.txt"],"scripts":{"prepare":"node -e \"require('fs').writeFileSync('prepare.txt', 'prepared')\""}}"#,
    );
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("shared-prepare", &spec)]);
    allow_builds(&workspace, &[&format!("shared-prepare@{spec}")]);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(workspace.join("node_modules/shared-prepare/prepare.txt").exists());

    let workspace_b = root.path().join("workspace-b");
    fs::create_dir(&workspace_b).expect("create second workspace");
    fs::copy(workspace.join(".npmrc"), workspace_b.join(".npmrc"))
        .expect("copy shared-store npmrc");
    fs::copy(workspace.join("pnpm-workspace.yaml"), workspace_b.join("pnpm-workspace.yaml"))
        .expect("copy shared-store workspace config");
    let workspace_b_yaml = fs::read_to_string(workspace_b.join("pnpm-workspace.yaml"))
        .expect("read second workspace config");
    let (workspace_b_yaml, _) = workspace_b_yaml
        .split_once("allowBuilds:")
        .expect("the first workspace has an allowBuilds block");
    fs::write(workspace_b.join("pnpm-workspace.yaml"), workspace_b_yaml)
        .expect("remove build approval from second workspace");
    write_dependencies(&workspace_b, &[("shared-prepare", &spec)]);

    let output = pnpm_at(&workspace_b)
        .with_arg("install")
        .output()
        .expect("install from warm store");
    dbg!(&output);
    assert!(!output.status.success(), "the unapproved warm-store install unexpectedly succeeded");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("ERR_PNPM_GIT_DEP_PREPARE_NOT_ALLOWED"),
        "stderr did not report the build-policy failure",
    );
    assert!(!workspace_b.join("node_modules/shared-prepare/prepare.txt").exists());

    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir.clone());
    let store_index_key = pnpm_store_dir::git_hosted_store_index_key(&spec, true);
    let store_index = pnpm_store_dir::StoreIndex::open_in(&store_dir).expect("open store index");
    let mut legacy_index = store_index
        .get(&store_index_key)
        .expect("read store index")
        .expect("prepared git package is indexed");
    assert_eq!(legacy_index.requires_prepare, Some(true));
    legacy_index.requires_prepare = None;
    store_index.set(&store_index_key, &legacy_index).expect("write legacy store index row");

    let workspace_c = root.path().join("workspace-c");
    fs::create_dir(&workspace_c).expect("create third workspace");
    fs::copy(workspace.join(".npmrc"), workspace_c.join(".npmrc"))
        .expect("copy shared-store npmrc");
    fs::write(workspace_c.join("pnpm-workspace.yaml"), workspace_b_yaml)
        .expect("write workspace config without build approval");
    write_dependencies(&workspace_c, &[("shared-prepare", &spec)]);

    let output = pnpm_at(&workspace_c)
        .with_arg("install")
        .output()
        .expect("install from legacy store");
    dbg!(&output);
    assert!(!output.status.success(), "the unapproved legacy-store install unexpectedly succeeded");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("ERR_PNPM_GIT_DEP_PREPARE_NOT_ALLOWED"),
        "stderr did not report the build-policy failure",
    );
    assert!(!workspace_c.join("node_modules/shared-prepare/prepare.txt").exists());

    drop((root, npmrc_info));
}

#[test]
fn type_git_dependency_reuses_side_effects_on_warm_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "git-side-effects");
    let lifecycle_log = root.path().join("git-side-effects-builds.log");
    let script = format!(
        r"require('fs').appendFileSync({}, 'built\n')",
        serde_json::to_string(&lifecycle_log).expect("serialize lifecycle log path"),
    );
    repo.write_file(
        "package.json",
        &json!({
            "name": "git-side-effects",
            "version": "1.0.0",
            "scripts": { "postinstall": format!("node -e {script:?}") },
        })
        .to_string(),
    );
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("git-side-effects", &spec)]);
    allow_builds(&workspace, &[&format!("git-side-effects@{spec}")]);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let builds_after_cold_install =
        fs::read_to_string(&lifecycle_log).expect("read cold-install lifecycle log");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&lifecycle_log).expect("read warm-install lifecycle log"),
        builds_after_cold_install,
        "the warm install must materialize cached side effects without rerunning postinstall",
    );

    drop((root, npmrc_info));
}

#[test]
fn git_dependency_is_built_on_isolated_reinstall() {
    assert_git_dependency_is_built_on_reinstall(None);
}

#[test]
fn git_dependency_is_built_on_hoisted_reinstall() {
    assert_git_dependency_is_built_on_reinstall(Some("hoisted"));
}

/// A git dependency installed under an alias is gated on its *manifest*
/// name, not the alias: `allowBuilds` has to name `<manifest name>@<spec>`
/// for `prepare` to run.
///
/// The lockfile keys the package the same way, and pnpm's
/// `preparePackage` builds the identity it checks from the fetched
/// `package.json` too, so the alias never enters the build policy.
#[test]
fn an_aliased_git_dependency_is_gated_on_its_manifest_name() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "hi");
    repo.write_file(
        "package.json",
        r#"{"name":"hi","version":"1.0.0","files":["package.json","prepare.txt"],"scripts":{"prepare":"node -e \"require('fs').writeFileSync('prepare.txt', 'prepared')\""}}"#,
    );
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("say-hi", &spec)]);
    allow_builds(&workspace, &[&format!("hi@{spec}")]);

    pacquet
        .with_args(["install"])
        .assert()
        .success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, ".", "say-hi"), format!("hi@{spec}"));
    assert!(
        workspace.join("node_modules/say-hi/prepare.txt").exists(),
        "the manifest-name allowBuilds entry must let `prepare` run under the alias",
    );

    drop((root, npmrc_info));
}

/// A dependency that pins its own package manager is prepared with that
/// package manager, provisioned by pnpm rather than expected on the host.
///
/// The pin is Yarn Classic, which no fixture can stand in for: pnpm
/// verifies an engine against npm's published signature before running
/// it, so the bytes have to be the real ones. The dependency's own
/// `prepare` script records the user agent it ran under, which is what
/// names the package manager that prepared it.
#[test]
fn a_git_dependency_is_prepared_with_the_package_manager_it_pins() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "pins-yarn");
    repo.write_file(
        "package.json",
        r#"{"name":"pins-yarn","version":"1.0.0","main":"index.js","packageManager":"yarn@1.22.22","scripts":{"prepare":"node record-user-agent.js"}}"#,
    );
    repo.write_file("index.js", "module.exports = 'ok'\n");
    repo.write_file(
        "record-user-agent.js",
        "require('fs').writeFileSync('prepared-by.txt', process.env.npm_config_user_agent || '')\n",
    );
    repo.write_file("yarn.lock", "# yarn lockfile v1\n");
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);

    write_dependencies(&workspace, &[("pins-yarn", &spec)]);
    allow_builds(&workspace, &[&format!("pins-yarn@{spec}")]);

    // The provisioning runs in a child pnpm, outside this project, so the
    // registry to provision from travels in the environment. Every
    // directory the engine could land in is pinned into the test's own
    // root, so the run cannot reach into the developer's — and so the
    // engine it installs can be found below.
    let output = pnpm_at(&workspace)
        .with_args(["install"])
        .with_env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url())
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_DATA_HOME", root.path().join("data"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
        .with_env("YARN_IGNORE_PATH", "1")
        .output()
        .expect("run pnpm install");
    dbg!(&output);
    assert_success(&output);

    let user_agent = fs::read_to_string(workspace.join("node_modules/pins-yarn/prepared-by.txt"))
        .expect("the pinned package manager should have run the dependency's prepare script");
    assert!(
        user_agent.starts_with("yarn/1.22.22"),
        "prepared by the pinned yarn, not {user_agent:?}",
    );
    // And it was pnpm's own Yarn that ran: the engine is in the store
    // this test pinned, which a host Yarn would have left empty.
    let engine_store = root
        .path()
        .join("pnpm-home")
        .join("package-manager-store");
    let provisioned = walkdir::WalkDir::new(&engine_store)
        .into_iter()
        .flatten()
        .any(|entry| entry.file_name() == "yarn.js");
    assert!(provisioned, "no provisioned yarn under {}", engine_store.display());

    drop((root, npmrc_info));
}

/// TS: `git-hosted repository is not added to the store if it fails to
/// be built` (`fromRepo.ts:354`).
///
/// The second install is the assertion: a package whose `prepare`
/// failed must not have been indexed, or the retry would find a
/// half-built package in the store and succeed.
#[test]
fn git_hosted_repository_is_not_added_to_the_store_if_it_fails_to_be_built() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "prepare-script-fails");
    repo.write_file(
        "package.json",
        r#"{"name":"prepare-script-fails","version":"1.0.0","main":"index.js","scripts":{"prepare":"node -e \"process.exit(1)\""}}"#,
    );
    repo.write_file("index.js", "module.exports = true\n");
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);

    write_dependencies(&workspace, &[("prepare-script-fails", &spec)]);
    allow_builds(&workspace, &[&format!("prepare-script-fails@{spec}")]);

    pacquet
        .with_args(["install"])
        .assert()
        .failure();
    pnpm_at(&workspace)
        .with_args(["install"])
        .assert()
        .failure();

    drop((root, npmrc_info));
}

fn assert_git_dependency_is_built_on_reinstall(node_linker: Option<&str>) {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "prepare-script-works");
    repo.write_file(
        "package.json",
        r#"{"name":"prepare-script-works","version":"1.0.0","files":["package.json","prepare.txt"],"scripts":{"prepare":"node -e \"require('fs').writeFileSync('prepare.txt', 'prepared')\""}}"#,
    );
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("prepare-script-works", &spec)]);
    allow_builds(&workspace, &[&format!("prepare-script-works@{spec}")]);
    if let Some(node_linker) = node_linker {
        append_workspace_yaml_key(&workspace, "nodeLinker", node_linker);
    }
    let marker = workspace.join("node_modules/prepare-script-works/prepare.txt");

    pacquet
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let marker_exists = marker.exists();
    eprintln!("MARKER: {}\nEXISTS: {marker_exists}\n", marker.display());
    assert!(!marker_exists, "the ignored initial install must not prepare the package");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_at(&workspace)
        .with_args(["install", "--config.prefer-frozen-lockfile=false"])
        .assert()
        .success();
    let marker_exists = marker.exists();
    eprintln!("MARKER: {}\nEXISTS: {marker_exists}\n", marker.display());
    assert!(marker_exists, "a fresh-resolution reinstall must prepare the package");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let marker_exists = marker.exists();
    eprintln!("MARKER: {}\nEXISTS: {marker_exists}\n", marker.display());
    assert!(marker_exists, "a frozen reinstall must materialize the prepared package");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
        .assert()
        .success();
    let marker_exists = marker.exists();
    eprintln!("MARKER: {}\nEXISTS: {marker_exists}\n", marker.display());
    assert!(!marker_exists, "--ignore-scripts must keep prepare output out of the install");

    drop((root, npmrc_info));
}

#[test]
fn explicitly_denied_git_preparation_keeps_source_and_separates_cached_builds() {
    let CommandTempCwd {
        pacquet: _,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "denied-prepare");
    repo.write_file("index.js", "module.exports = 42");
    repo.write_file("package.json", &json!({
        "name": "denied-prepare", "version": "1.0.0",
        "scripts": {
            "prepare": r#"node -e "require('fs').writeFileSync('prepare.txt', 'prepared')""#,
            "postinstall": r#"node -e "require('fs').writeFileSync('postinstall.txt', 'built')""#,
        },
    }).to_string());
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("denied-prepare", &spec)]);
    let config_path = workspace.join("pnpm-workspace.yaml");
    let config = fs::read_to_string(&config_path).expect("read workspace config");
    for allowed in [false, false, true, false] {
        let modules = workspace.join("node_modules");
        if modules.exists() {
            fs::remove_dir_all(&modules).expect("remove node_modules");
        }
        let key =
            if allowed { format!("denied-prepare@{spec}") } else { "denied-prepare".to_string() };
        fs::write(&config_path, format!("{config}\nallowBuilds:\n  '{key}': {allowed}\n"))
            .expect("write build policy");
        pnpm_at(&workspace)
            .with_arg("install")
            .assert()
            .success();
        assert_eq!(
            fs::read_to_string(modules.join("denied-prepare/index.js")).unwrap(),
            "module.exports = 42",
        );
        assert_eq!(modules.join("denied-prepare/prepare.txt").exists(), allowed);
        assert_eq!(modules.join("denied-prepare/postinstall.txt").exists(), allowed);
    }
    drop((root, npmrc_info));
}

/// A git dependency is prepared by running `pnpm install` in its
/// checkout, where nobody can approve the build scripts of its own
/// dependencies. That install skips those builds instead of failing with
/// `ERR_PNPM_IGNORED_BUILDS`, so the dependency is still prepared.
///
/// The pnpm under test goes first on `PATH`, so it is also the one that
/// prepares the dependency. The nested install runs outside this project,
/// so its registry, store, and cache travel in the environment.
#[test]
fn a_git_dependency_is_prepared_when_its_own_dependencies_have_unapproved_builds() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "has-unapproved-build");
    repo.write_file(
        "package.json",
        r#"{"name":"has-unapproved-build","version":"1.0.0","main":"index.js","scripts":{"prepare":"node record-build.js"},"devDependencies":{"@pnpm.e2e/pre-and-postinstall-scripts-example":"1.0.0"}}"#,
    );
    repo.write_file(
        "record-build.js",
        "const fs = require('fs')\n\
         const dependency = 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example'\n\
         if (!fs.existsSync(dependency + '/package.json')) throw new Error('the dependency was not installed')\n\
         fs.writeFileSync('dependency-was-built.txt', String(fs.existsSync(dependency + '/generated-by-postinstall.js')))\n",
    );
    repo.write_file("index.js", "module.exports = 'ok'\n");
    repo.write_file("pnpm-lock.yaml", "");
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);

    write_dependencies(&workspace, &[("has-unapproved-build", &spec)]);
    allow_builds(&workspace, &[&format!("has-unapproved-build@{spec}")]);

    let pnpm_dir = assert_cmd::cargo::cargo_bin("pnpm")
        .parent()
        .expect("the pnpm binary has a parent directory")
        .to_path_buf();
    let path = std::env::join_paths(
        std::iter::once(pnpm_dir)
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .expect("join PATH");

    let output = pacquet
        .with_args(["install"])
        .with_env("PATH", path)
        .with_env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url())
        .with_env("PNPM_CONFIG_STORE_DIR", &npmrc_info.store_dir)
        .with_env("PNPM_CONFIG_CACHE_DIR", &npmrc_info.cache_dir)
        .with_env("PNPM_CONFIG_FROZEN_LOCKFILE", "false")
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_DATA_HOME", root.path().join("data"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
        .output()
        .expect("run pnpm install");
    dbg!(&output);
    assert_success(&output);

    let dependency_was_built = fs::read_to_string(workspace.join(
        "node_modules/has-unapproved-build/dependency-was-built.txt",
    ))
    .expect("the dependency should have been prepared");
    assert_eq!(dependency_was_built, "false", "the unapproved build must stay skipped");

    drop((root, npmrc_info));
}
