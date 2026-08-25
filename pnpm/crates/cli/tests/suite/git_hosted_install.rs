//! Installing a dependency hosted in a git repository.
//!
//! Ports the install half of
//! `TypeScript repo: installing/deps-installer/test/install/fromRepo.ts`
//! plus the git-hosted `prepare` case from `lifecycleScripts.ts:311`.
//!
//! Upstream points these at real repositories on github.com. Here each
//! test builds its own repo on disk and installs it over `git+file://`
//! ([`GitRepoFixture`]), so the git install path runs end to end without
//! reaching the network — the same technique upstream's own
//! `createGitPreparePackage` uses. What that trades away is the *host*
//! identity: a `github:`/`gitlab:`/`bitbucket:` spec resolves to the
//! host's archive URL (a `gitHosted: true` tarball resolution), while a
//! `file:` repo has no archive endpoint and resolves to `type: git`.
//! The host-archive shape is pinned at the resolver level in
//! `pnpm-resolving-git-resolver`.

use crate::_utils;

use std::{fmt::Write as _, fs, path::Path, process::Command};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_testing_utils::{bin::CommandTempCwd, git_repo::GitRepoFixture};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use _utils::{
    append_workspace_yaml_key, assert_success, importer_specifier, importer_version,
    ndjson_records, read_lockfile, read_manifest, write_manifest_value,
};

/// The `hi` package upstream installs under the `say-hi` alias. Two bin
/// names under one script make bin linking observable, and the package
/// name differs from every alias the tests give it.
fn say_hi_repo(root: &Path) -> (GitRepoFixture, String) {
    let repo = GitRepoFixture::init(root, "hi");
    repo.write_file(
        "package.json",
        r#"{"name":"hi","version":"1.0.0","main":"index.js","bin":{"hi":"index.js","szia":"index.js"}}"#,
    );
    repo.write_file("index.js", "#!/usr/bin/env node\nmodule.exports = 'Hi'\n");
    let commit = repo.commit("init");
    (repo, commit)
}

/// A single-package repo named after `name`, at `version`.
fn simple_repo(root: &Path, name: &str, version: &str) -> (GitRepoFixture, String) {
    let repo = GitRepoFixture::init(root, name);
    repo.write_file(
        "package.json",
        &format!(r#"{{"name":"{name}","version":"{version}","main":"index.js"}}"#),
    );
    repo.write_file("index.js", "module.exports = true\n");
    let commit = repo.commit("init");
    (repo, commit)
}

/// A fresh `pnpm` invocation in `workspace`.
///
/// [`CommandTempCwd`] hands out one prepared `Command`; follow-up runs
/// against the same project build their own. The registry, store, and
/// cache all come from the config files the harness already wrote into
/// the workspace, so nothing else has to be re-threaded.
fn pnpm_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// Write `project/package.json` with `dependencies` set to `deps`.
fn write_dependencies(project: &Path, deps: &[(&str, &str)]) {
    let dependencies: serde_json::Map<String, Value> =
        deps.iter().map(|(name, spec)| ((*name).to_string(), json!(spec))).collect();
    write_manifest_value(
        project,
        &json!({ "name": "project", "version": "1.0.0", "dependencies": dependencies }),
    );
}

/// The lone `packages:` entry whose key names `name`, as a
/// `(package_key, metadata)` pair.
fn sole_package<'a>(
    lockfile: &'a Lockfile,
    name: &str,
) -> (String, &'a pnpm_lockfile::PackageMetadata) {
    let prefix = format!("{name}@");
    let mut matches = lockfile
        .packages
        .as_ref()
        .expect("lockfile has packages")
        .iter()
        .filter(|(key, _)| key.to_string().starts_with(&prefix))
        .map(|(key, metadata)| (key.to_string(), metadata));
    let found = matches.next().unwrap_or_else(|| panic!("no packages entry for {name}"));
    assert!(matches.next().is_none(), "expected exactly one packages entry for {name}");
    found
}

/// The `type: git` resolution of the lone `packages:` entry for `name`.
fn git_resolution<'a>(lockfile: &'a Lockfile, name: &str) -> &'a pnpm_lockfile::GitResolution {
    match &sole_package(lockfile, name).1.resolution {
        LockfileResolution::Git(git) => git,
        other => panic!("expected a git resolution for {name}, got {other:?}"),
    }
}

/// TS: `from a git repo` (`fromRepo.ts:174`).
///
/// Upstream reaches github over `git+ssh://` and skips itself on CI;
/// the `file:` repo here resolves through the same non-host branch
/// (`LockfileResolution::Git`) without needing an SSH agent.
#[test]
fn install_from_a_git_repo() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let (repo, commit) = simple_repo(root.path(), "is-negative", "1.0.0");
    write_dependencies(&workspace, &[("is-negative", &repo.git_url_at(&commit))]);

    pacquet.with_args(["install"]).assert().success();

    assert!(workspace.join("node_modules/is-negative/package.json").exists());
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let resolution = git_resolution(&lockfile, "is-negative");
    assert_eq!(resolution.commit, commit);
    assert_eq!(resolution.repo, repo.file_url());
    assert_eq!(resolution.path, None);
    // A non-host git dep with no alias records the bare `git+...#<commit>`
    // ref in the importer, not `is-negative@git+...` — byte-for-byte what
    // pnpm 11 writes.
    assert_eq!(importer_version(&lockfile, ".", "is-negative"), repo.git_url_at(&commit));

    drop((root, npmrc_info));
}

/// No pnpm version computes an integrity for a git checkout, yet
/// lockfiles in the wild record one. Installing must not choke on it, and
/// must not write it back — nothing verifies a git checkout against a
/// hash. See <https://github.com/pnpm/pnpm/issues/13042>.
#[test]
fn install_from_a_git_repo_whose_lockfile_records_an_integrity() {
    const INTEGRITY: &str = "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==";

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let (repo, commit) = simple_repo(root.path(), "is-negative", "1.0.0");
    write_dependencies(&workspace, &[("is-negative", &repo.git_url_at(&commit))]);

    pacquet.with_args(["install"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let written = fs::read_to_string(&lockfile_path).expect("read lockfile");
    let with_integrity = written.replace(", repo: ", &format!(", integrity: {INTEGRITY}, repo: "));
    assert_ne!(with_integrity, written, "the git resolution must have a `repo` key to edit");
    fs::write(&lockfile_path, &with_integrity).expect("write lockfile");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(workspace.join("node_modules/is-negative/package.json").exists());
    let lockfile = read_lockfile(&lockfile_path);
    assert_eq!(git_resolution(&lockfile, "is-negative").integrity, None, "read back as a hash");

    // Adding a dependency is what next rewrites the lockfile; the hash
    // leaves with that write rather than provoking one of its own.
    let (other, other_commit) = simple_repo(root.path(), "is-positive", "1.0.0");
    write_dependencies(
        &workspace,
        &[
            ("is-negative", &repo.git_url_at(&commit)),
            ("is-positive", &other.git_url_at(&other_commit)),
        ],
    );
    pnpm_at(&workspace).with_arg("install").assert().success();

    let rewritten = fs::read_to_string(&lockfile_path).expect("read rewritten lockfile");
    assert!(!rewritten.contains(INTEGRITY), "the rewritten lockfile still advertises the hash");
    let lockfile = read_lockfile(&lockfile_path);
    assert_eq!(git_resolution(&lockfile, "is-negative").commit, commit);

    drop((root, npmrc_info));
}

/// TS: `from a github repo with different name via named installation`
/// (`fromRepo.ts:61`).
///
/// The alias is the point: the manifest and the importer entry key on
/// `say-hi`, while the package resolves to `hi` — so the importer's
/// version keeps the `hi@` prefix, the `pnpm:root` event reports both
/// names, and both of the package's bins are linked under their own
/// names rather than the alias.
#[test]
fn install_from_a_git_repo_with_a_different_name_via_named_installation() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let (repo, commit) = say_hi_repo(root.path());
    let spec = repo.git_url_at(&commit);

    let output = pacquet
        .with_args(["add", &format!("say-hi@{spec}"), "--reporter=ndjson"])
        .output()
        .expect("run pnpm add");
    assert_success(&output);

    assert_eq!(
        read_manifest(&workspace)["dependencies"],
        json!({ "say-hi": spec }),
        "the git specifier is saved verbatim under the alias",
    );

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_specifier(&lockfile, ".", "say-hi"), spec);
    assert_eq!(importer_version(&lockfile, ".", "say-hi"), format!("hi@{spec}"));

    let added = ndjson_records(&output)
        .into_iter()
        .filter_map(|record| {
            (record.get("name").and_then(Value::as_str) == Some("pnpm:root"))
                .then(|| record.get("added").cloned())
                .flatten()
        })
        .find(|added| added.get("name").and_then(Value::as_str) == Some("say-hi"))
        .expect("a pnpm:root `added` record for say-hi");
    assert_eq!(added["realName"], "hi");
    assert_eq!(added["version"], "1.0.0");
    assert_eq!(added["dependencyType"], "prod");

    for bin in ["hi", "szia"] {
        assert!(
            workspace.join("node_modules/.bin").join(bin).exists(),
            "{bin} should be linked into node_modules/.bin",
        );
    }

    drop((root, npmrc_info));
}

/// TS: `from a github repo with different name` (`fromRepo.ts:105`).
///
/// Same shape as the named-installation case, reached through a
/// manifest that already declares the alias rather than through `add` —
/// upstream keeps both because the two entered the installer by
/// different routes.
#[test]
fn install_from_a_git_repo_with_a_different_name() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let (repo, commit) = say_hi_repo(root.path());
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("say-hi", &spec)]);

    pacquet.with_args(["install"]).assert().success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_specifier(&lockfile, ".", "say-hi"), spec);
    assert_eq!(importer_version(&lockfile, ".", "say-hi"), format!("hi@{spec}"));
    assert_eq!(sole_package(&lockfile, "hi").1.version.as_deref(), Some("1.0.0"));

    let linked = workspace.join("node_modules/say-hi/package.json");
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&linked).expect("read the linked manifest"))
            .expect("parse the linked manifest");
    assert_eq!(manifest["name"], "hi", "the alias directory holds the real package");

    drop((root, npmrc_info));
}

/// TS: `re-adding a git repo with a different tag` (`fromRepo.ts:276`).
///
/// Each tag is a distinct commit, so re-adding must re-resolve to the
/// second one and leave exactly one `packages:` entry behind — a stale
/// entry for the first tag would mean the old commit is still installed.
#[test]
fn re_adding_a_git_repo_with_a_different_tag() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "is-negative");
    repo.write_file("package.json", r#"{"name":"is-negative","version":"1.0.0"}"#);
    let first_commit = repo.commit("1.0.0");
    repo.tag("1.0.0");
    repo.write_file("package.json", r#"{"name":"is-negative","version":"1.0.1"}"#);
    let second_commit = repo.commit("1.0.1");
    repo.tag("1.0.1");
    assert_ne!(first_commit, second_commit);

    let installed_version = |workspace: &Path| -> String {
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(workspace.join("node_modules/is-negative/package.json"))
                .expect("read the installed manifest"),
        )
        .expect("parse the installed manifest");
        manifest["version"].as_str().expect("version is a string").to_string()
    };

    write_dependencies(&workspace, &[("is-negative", &repo.git_url_at("1.0.0"))]);
    pacquet.with_args(["install"]).assert().success();

    assert_eq!(installed_version(&workspace), "1.0.0");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(git_resolution(&lockfile, "is-negative").commit, first_commit);

    write_dependencies(&workspace, &[("is-negative", &repo.git_url_at("1.0.1"))]);
    pnpm_at(&workspace).with_args(["install"]).assert().success();

    assert_eq!(installed_version(&workspace), "1.0.1");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(git_resolution(&lockfile, "is-negative").commit, second_commit);
    assert_eq!(
        importer_specifier(&lockfile, ".", "is-negative"),
        repo.git_url_at("1.0.1"),
        "the tag the user wrote is preserved, not the commit it resolved to",
    );

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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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

    pacquet.with_args(["install"]).assert().failure();
    pnpm_at(&workspace).with_args(["install"]).assert().failure();

    drop((root, npmrc_info));
}

/// TS: `from subdirectories of a git repo` (`fromRepo.ts:366`).
///
/// Two packages out of one repo: each `#path:` selects its own
/// subdirectory, and the two must not collide even though they share a
/// repo and a commit.
#[test]
fn install_from_subdirectories_of_a_git_repo() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "test-git-subfolder-fetch");
    repo.write_file("package.json", r#"{"name":"monorepo-root","version":"0.0.0"}"#);
    for name in ["simple-react-app", "simple-express-server"] {
        repo.write_file(
            &format!("packages/{name}/package.json"),
            &format!(r#"{{"name":"@my-namespace/{name}","version":"1.0.0","main":"index.js"}}"#),
        );
        repo.write_file(&format!("packages/{name}/index.js"), "module.exports = true\n");
    }
    let commit = repo.commit("init");

    let react_spec = format!("{}&path:/packages/simple-react-app", repo.git_url_at(&commit));
    let express_spec = format!("{}&path:/packages/simple-express-server", repo.git_url_at(&commit));
    write_dependencies(
        &workspace,
        &[
            ("@my-namespace/simple-react-app", &react_spec),
            ("@my-namespace/simple-express-server", &express_spec),
        ],
    );

    pacquet.with_args(["install"]).assert().success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    for name in ["simple-react-app", "simple-express-server"] {
        let package = format!("@my-namespace/{name}");
        assert!(
            workspace.join("node_modules").join(&package).join("package.json").exists(),
            "{package} should be installed",
        );
        let resolution = git_resolution(&lockfile, &package);
        assert_eq!(resolution.commit, commit);
        assert_eq!(resolution.path.as_deref(), Some(format!("/packages/{name}").as_str()));
    }

    drop((root, npmrc_info));
}

/// TS: `no hash character for github subdirectory install`
/// (`fromRepo.ts:389`).
///
/// `#path:/&<ref>` puts the ref *after* the `path:` parameter with no
/// second `#`, so the whole fragment has to be split on `&` rather than
/// read as "everything after the hash is the committish".
#[test]
fn no_hash_character_for_subdirectory_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "only-allow");
    repo.write_file("package.json", r#"{"name":"only-allow","version":"1.2.1","main":"index.js"}"#);
    repo.write_file("index.js", "module.exports = true\n");
    let commit = repo.commit("init");
    repo.tag("v1.2.1");

    write_dependencies(
        &workspace,
        &[("only-allow", &format!("git+{}#path:/&v1.2.1", repo.file_url()))],
    );

    pacquet.with_args(["install"]).assert().success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let resolution = git_resolution(&lockfile, "only-allow");
    assert_eq!(resolution.commit, commit, "`v1.2.1` after the `&` is the committish");
    assert_eq!(resolution.path.as_deref(), Some("/"), "`path:/` is the repo root");

    drop((root, npmrc_info));
}

/// TS: `run prepare script for git-hosted dependencies`
/// (`lifecycleScripts.ts:311`).
///
/// A git dependency has no published tarball, so pnpm builds it on the
/// way in: the install lifecycle runs once for the checkout, `prepare`
/// packs it, and the lifecycle runs again for the installed package.
#[test]
fn run_prepare_script_for_git_hosted_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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

    pacquet.with_args(["install"]).assert().success();

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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "shared-prepare");
    repo.write_file(
        "package.json",
        r#"{"name":"shared-prepare","version":"1.0.0","files":["package.json","prepare.txt"],"scripts":{"prepare":"node -e \"require('fs').writeFileSync('prepare.txt', 'prepared')\""}}"#,
    );
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("shared-prepare", &spec)]);
    allow_builds(&workspace, &[&format!("shared-prepare@{spec}")]);

    pacquet.with_arg("install").assert().success();
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

    let output =
        pnpm_at(&workspace_b).with_arg("install").output().expect("install from warm store");
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

    let output =
        pnpm_at(&workspace_c).with_arg("install").output().expect("install from legacy store");
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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

    pacquet.with_arg("install").assert().success();
    let builds_after_cold_install =
        fs::read_to_string(&lifecycle_log).expect("read cold-install lifecycle log");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

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

fn assert_git_dependency_is_built_on_reinstall(node_linker: Option<&str>) {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
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
    pnpm_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
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

// TS: `from a github repo` / `from a github repo through URL`
// (`fromRepo.ts:31`, `fromRepo.ts:48`). The forge spelling only changes
// normalization; a local git URL exercises the alias-less add path without
// depending on a public service.
#[test]
fn add_from_a_git_url_without_an_alias() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let (repo, commit) = simple_repo(root.path(), "is-negative", "1.0.0");
    let spec = repo.git_url_at(&commit);

    pacquet.with_args(["add", &spec]).assert().success();

    assert_eq!(read_manifest(&workspace)["dependencies"], json!({ "is-negative": spec }));
    let manifest_path = workspace.join("node_modules/is-negative/package.json");
    eprintln!("MANIFEST: {}\n", manifest_path.display());
    assert!(manifest_path.exists());

    drop((root, npmrc_info));
}

#[test]
fn aliasless_git_add_rejects_an_invalid_manifest_name_without_mutating_the_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "invalid-package-name");
    repo.write_file("package.json", r#"{"name":"../invalid","version":"1.0.0","main":"index.js"}"#);
    repo.write_file("index.js", "module.exports = true\n");
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_manifest_value(&workspace, &json!({ "name": "project", "version": "1.0.0" }));
    let manifest_before =
        fs::read_to_string(workspace.join("package.json")).expect("read manifest");

    let output = pacquet.with_args(["add", &spec]).output().expect("run pnpm add");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_INVALID_PACKAGE_NAME"), "stderr:\n{stderr}");
    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("reread manifest"),
        manifest_before,
    );

    drop((root, npmrc_info));
}

// TS: `should not update when adding unrelated dependency`
// (`fromRepo.ts:323`).
#[test]
fn adding_an_unrelated_dependency_reuses_the_locked_git_commit() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "moving-git-dep");
    repo.write_file(
        "package.json",
        r#"{"name":"moving-git-dep","version":"1.0.0","main":"index.js"}"#,
    );
    repo.write_file("index.js", "module.exports = 1\n");
    let first_commit = repo.commit("first");
    let spec = repo.git_url_at("main");
    write_dependencies(&workspace, &[("moving-git-dep", &spec)]);
    pacquet.with_args(["install"]).assert().success();

    repo.write_file("index.js", "module.exports = 2\n");
    let second_commit = repo.commit("second");
    assert_ne!(first_commit, second_commit);
    pnpm_at(&workspace).with_args(["add", "@pnpm.e2e/abc"]).assert().success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(git_resolution(&lockfile, "moving-git-dep").commit, first_commit);

    drop((root, npmrc_info));
}

// TS: `a subdependency is from a github repo with different name`
// (`fromRepo.ts:150`) and `don't fail when peer dependency is fetched from
// GitHub` (`peerDependencies.ts:30`).
#[test]
fn registry_dependency_can_alias_a_git_dependency_that_provides_a_peer() {
    let fixture = CommandTempCwd::init();
    let (repo, commit) = say_hi_repo(fixture.root.path());
    let spec = repo.git_url_at(&commit);
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } = fixture
        .add_mocked_registry_with_substitutions(&[(
            "github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd",
            &spec,
        )]);
    append_workspace_yaml_key(&workspace, "blockExoticSubdeps", false);

    pacquet.with_args(["add", "@pnpm.e2e/has-aliased-git-dependency"]).assert().success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let parent: pnpm_lockfile::PkgNameVerPeer =
        "@pnpm.e2e/has-aliased-git-dependency@1.0.0".parse().expect("parse parent key");
    let snapshot = &lockfile.snapshots.as_ref().expect("lockfile has snapshots")[&parent];
    assert_eq!(
        snapshot
            .dependencies
            .as_ref()
            .expect("parent has dependencies")
            .get(&"say-hi".parse().expect("parse dependency name"))
            .expect("say-hi dependency")
            .to_string(),
        format!("hi@{spec}"),
    );
    for bin in ["hi", "szia"] {
        let bin_path = workspace
            .join("node_modules/@pnpm.e2e/has-aliased-git-dependency/node_modules/.bin")
            .join(bin);
        eprintln!("BIN: {}\n", bin_path.display());
        assert!(bin_path.exists(), "{bin} should be linked for the registry package");
    }

    drop((root, npmrc_info));
}

/// A git specifier names a repository, not a package, so the name the
/// peer is matched on lives only in the repo's own manifest — read
/// during resolution, early enough for the hoist that
/// `resolvePeersFromWorkspaceRoot` runs
/// (<https://github.com/pnpm/pnpm/issues/13351>).
#[test]
fn an_aliased_git_root_dependency_provides_another_importers_peer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "scoped-peer");
    repo.write_file("package.json", r#"{"name":"@scoped/peer","version":"1.0.0"}"#);
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_manifest_value(
        &workspace,
        &json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "vendored-peer": spec },
        }),
    );
    let app = workspace.join("app");
    fs::create_dir(&app).expect("create the app project");
    write_manifest_value(
        &app,
        &json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "@having/scoped-peer": "1.0.0" },
        }),
    );
    append_workspace_yaml_key(&workspace, "packages", "[app]");

    pacquet.with_args(["install"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = read_lockfile(&lockfile_path);
    let git_key = format!("@scoped/peer@{spec}");
    assert_eq!(importer_version(&lockfile, ".", "vendored-peer"), git_key);
    assert_eq!(
        importer_version(&lockfile, "app", "@having/scoped-peer"),
        format!("1.0.0({git_key})"),
    );
    assert_eq!(
        sole_package(&lockfile, "@scoped/peer").0,
        git_key,
        "the peer must be the root's git dep, not a second copy off the registry",
    );

    // Every install after the first re-resolves with the prior lockfile
    // in hand, which is the shape a real workspace spends its life in.
    let first_install = fs::read(&lockfile_path).expect("read the lockfile");
    pnpm_at(&workspace).with_args(["install", "--no-prefer-frozen-lockfile"]).assert().success();
    assert_eq!(
        fs::read(&lockfile_path).expect("reread the lockfile"),
        first_install,
        "re-resolving against the recorded lockfile must not move the peer",
    );

    drop((root, npmrc_info));
}

// TS: `updating package that has a github-hosted dependency`
// (`lockfile.ts:600`).
#[test]
fn updating_a_registry_package_that_has_a_git_dependency() {
    let fixture = CommandTempCwd::init();
    let (repo, commit) = simple_repo(fixture.root.path(), "is-positive", "1.0.0");
    let spec = repo.git_url_at(&commit);
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        fixture.add_mocked_registry_with_substitutions(&[("kevva/is-positive", &spec)]);
    append_workspace_yaml_key(&workspace, "blockExoticSubdeps", false);

    pacquet.with_args(["add", "@pnpm.e2e/has-github-dep@1"]).assert().success();
    pnpm_at(&workspace).with_args(["add", "@pnpm.e2e/has-github-dep@latest"]).assert().success();

    assert_eq!(read_manifest(&workspace)["dependencies"]["@pnpm.e2e/has-github-dep"], "^2.0.0");

    drop((root, npmrc_info));
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "hi");
    repo.write_file(
        "package.json",
        r#"{"name":"hi","version":"1.0.0","files":["package.json","prepare.txt"],"scripts":{"prepare":"node -e \"require('fs').writeFileSync('prepare.txt', 'prepared')\""}}"#,
    );
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    write_dependencies(&workspace, &[("say-hi", &spec)]);
    allow_builds(&workspace, &[&format!("hi@{spec}")]);

    pacquet.with_args(["install"]).assert().success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, ".", "say-hi"), format!("hi@{spec}"));
    assert!(
        workspace.join("node_modules/say-hi/prepare.txt").exists(),
        "the manifest-name allowBuilds entry must let `prepare` run under the alias",
    );

    drop((root, npmrc_info));
}

/// The store index is shared with the TypeScript CLI, which keys a git
/// dependency's row by the bare `git+…#<commit>` resolution id. Keying
/// it by the lockfile's `<name>@git+…` form instead leaves a store
/// warmed by one stack cold for the other
/// ([#13365](https://github.com/pnpm/pnpm/issues/13365)).
#[test]
fn a_git_dependency_is_indexed_under_the_bare_resolution_id() {
    let fixture = CommandTempCwd::init();
    let (repo, commit) = say_hi_repo(fixture.root.path());
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } = fixture.add_mocked_registry();
    write_dependencies(&workspace, &[("hi", &repo.git_url_at(&commit))]);

    pacquet.with_args(["install"]).assert().success();

    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir.clone());
    let keys = pnpm_store_dir::StoreIndex::open_readonly_in(&store_dir)
        .expect("open the store index")
        .keys()
        .expect("read the store index keys");
    let pkg_id = repo.git_url_at(&commit);
    assert!(
        keys.iter().any(|key| key == &format!("{pkg_id}\tbuilt")),
        "no store-index row keyed by the bare resolution id {pkg_id:?}: {keys:?}",
    );
    assert!(
        !keys.iter().any(|key| key.starts_with("hi@")),
        "the lockfile-shaped `<name>@<id>` key must not reach the store index: {keys:?}",
    );

    drop((root, npmrc_info));
}

/// Append an `allowBuilds` block to the `pnpm-workspace.yaml` the
/// harness wrote, opting the listed `<name>@<specifier>` keys into
/// running lifecycle scripts.
fn allow_builds(workspace: &Path, keys: &[&str]) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = std::fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    assert!(
        !yaml.contains("allowBuilds:"),
        "pnpm-workspace.yaml already has an `allowBuilds:` key — update this helper",
    );
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("allowBuilds:\n");
    for key in keys {
        // The keys are URLs, so they carry `:` and must be quoted to
        // stay a single YAML scalar.
        writeln!(yaml, "  {key:?}: true").expect("format allowBuilds entry");
    }
    std::fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
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
    let engine_store = root.path().join("pnpm-home").join("package-manager-store");
    let provisioned = walkdir::WalkDir::new(&engine_store)
        .into_iter()
        .flatten()
        .any(|entry| entry.file_name() == "yarn.js");
    assert!(provisioned, "no provisioned yarn under {}", engine_store.display());

    drop((root, npmrc_info));
}

/// A git dependency is packed by pnpm rather than by its publisher, so
/// its `files` field is what decides the installed file set.
#[test]
fn files_field_of_a_git_dependency_does_not_match_at_depth() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "packs-its-own-src");
    repo.write_file(
        "package.json",
        r#"{"name":"packs-its-own-src","version":"1.0.0","main":"src/index.js","files":["src"]}"#,
    );
    repo.write_file("src/index.js", "module.exports = true\n");
    repo.write_file("example/src/App.js", "module.exports = 'example'\n");
    let commit = repo.commit("init");
    write_dependencies(&workspace, &[("packs-its-own-src", &repo.git_url_at(&commit))]);

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

    let installed = workspace.join("node_modules/packs-its-own-src");
    assert!(installed.join("src/index.js").is_file(), "the published src ships");
    assert!(!installed.join("example").exists(), "the repository's example app is not published");

    drop((root, npmrc_info));
}
