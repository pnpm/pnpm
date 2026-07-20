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
//! `pacquet-resolving-git-resolver`.

pub mod _utils;

use std::{fmt::Write as _, path::Path, process::Command};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{Lockfile, LockfileResolution};
use pacquet_testing_utils::{bin::CommandTempCwd, git_repo::GitRepoFixture};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use _utils::{
    assert_success, importer_specifier, importer_version, ndjson_records, read_lockfile,
    read_manifest, write_manifest_value,
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
) -> (String, &'a pacquet_lockfile::PackageMetadata) {
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
fn git_resolution<'a>(lockfile: &'a Lockfile, name: &str) -> &'a pacquet_lockfile::GitResolution {
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

mod known_failures {
    //! Git-hosted install cases blocked on behavior pacquet hasn't
    //! built or reconciled yet. Each stubs the not-yet-implemented
    //! subject under test through
    //! [`pacquet_testing_utils::allow_known_failure`] so the test exits
    //! early rather than masking a real bug.

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn alias_less_git_add() -> KnownResult<()> {
        Err(KnownFailure::new(
            "`pnpm add <git-specifier>` without an explicit `<alias>@` \
             prefix is not supported. `add`'s `split_name_spec` treats \
             the whole argument as a package name, so \
             `pnpm add kevva/is-negative` / \
             `pnpm add https://github.com/kevva/is-negative` ask the \
             registry for a package literally named by the specifier \
             and fail with ERR_PNPM_INVALID_PACKAGE_NAME. Upstream \
             resolves the repo first and takes the name from the \
             fetched manifest. The aliased form \
             (`pnpm add say-hi@github:zkochan/hi`) and every \
             manifest-declared git dependency already work — see the \
             real tests in this file.",
        ))
    }

    fn git_dep_version_in_root_log() -> KnownResult<()> {
        Err(KnownFailure::new(
            "The `pnpm:root` `added` event reports a git dependency's \
             `version` as the specifier's version slot — the \
             `git+<repo>#<commit>` URL — where upstream reports the \
             package's own version (`1.0.0`), which the default \
             reporter renders as `+ hi 1.0.0`. Pacquet prints \
             `+ hi git+file:///...#<commit>`. The version lives in the \
             lockfile's `packages:` entry, which \
             `symlink_direct_dependencies` does not receive.",
        ))
    }

    fn git_ref_relocked_on_every_resolution() -> KnownResult<()> {
        Err(KnownFailure::new(
            "A git dependency's locked commit is not reused from the \
             wanted lockfile: every resolution pass re-runs \
             `git ls-remote` and relocks whatever the ref points at \
             now. For a moving ref (`#main`, a floating tag) an \
             unrelated `pnpm add` therefore bumps the git dependency's \
             commit, which upstream's test pins as the thing that must \
             not happen — verified against pnpm 11.13.1, which keeps \
             the locked commit after the branch moves. The reuse map \
             built in `install_with_fresh_lockfile` keys on integrity, \
             so git resolutions never enter it.",
        ))
    }

    fn git_dependency_of_a_registry_fixture() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Needs a registry fixture that declares a git dependency. A \
             fixture's `package.json` is static, so its git specifier \
             would have to name a repository that exists at test time — \
             i.e. a real forge URL, which would make the suite depend \
             on github.com being reachable. The install-level git tests \
             in this file build a repo per test and reference it by \
             `git+file://`, a URL no committed fixture can know. \
             Unblocking this needs the fixture builder in \
             `pnpr-fixtures` to support substituting a per-run repo URL \
             into a fixture manifest.",
        ))
    }

    /// TS: `from a github repo` (`fromRepo.ts:31`) — `pnpm add
    /// kevva/is-negative`, whose saved specifier is `github:kevva/is-negative`.
    #[test]
    fn add_from_a_github_repo_shorthand() {
        allow_known_failure!(alias_less_git_add());
    }

    /// TS: `from a github repo through URL` (`fromRepo.ts:48`) — `pnpm add
    /// https://github.com/kevva/is-negative`.
    #[test]
    fn add_from_a_github_repo_through_url() {
        allow_known_failure!(alias_less_git_add());
    }

    /// The `pnpm:root` `added.version` half of TS `from a github repo
    /// with different name via named installation` (`fromRepo.ts:61`),
    /// which expects `version: '1.0.0'`. The alias half — `name`,
    /// `realName`, `dependencyType` — is asserted by the real test.
    #[test]
    fn root_log_reports_the_package_version_for_a_git_dependency() {
        allow_known_failure!(git_dep_version_in_root_log());
    }

    /// TS: `should not update when adding unrelated dependency`
    /// (`fromRepo.ts:323`). Adding an unrelated registry dependency
    /// must leave a branch-pinned git dependency on its locked commit.
    #[test]
    fn should_not_update_when_adding_unrelated_dependency() {
        allow_known_failure!(git_ref_relocked_on_every_resolution());
    }

    /// TS: `a subdependency is from a github repo with different name`
    /// (`fromRepo.ts:150`), which installs
    /// `@pnpm.e2e/has-aliased-git-dependency` — a registry package whose
    /// own `dependencies` alias a github repo.
    #[test]
    fn a_subdependency_is_from_a_git_repo_with_a_different_name() {
        allow_known_failure!(git_dependency_of_a_registry_fixture());
    }

    /// TS: `updating package that has a github-hosted dependency`
    /// (`lockfile.ts:600`), which re-adds `@pnpm.e2e/has-github-dep` at
    /// a second version to prove the range-comparison path tolerates a
    /// non-semver pin among the dependency's own deps.
    #[test]
    fn updating_package_that_has_a_git_hosted_dependency() {
        allow_known_failure!(git_dependency_of_a_registry_fixture());
    }
}
