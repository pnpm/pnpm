//! Verify that every pnpm path that selects a package's files (`pack`,
//! `pnpm-fs-packlist`, `DirectoryFetcher`, `GitFetcher`, `patch-commit`,
//! `deploy`, and the injected-copy install and sync writers) agrees on the
//! selected file and link set (pnpm/tasks#223).

use crate::_utils::{WorkspaceFixture, assert_success};
use flate2::read::GzDecoder;
use pnpm_directory_fetcher::DirectoryFetcher;
use pnpm_git_fetcher::{
    GitFetcher, GitSource, GitSourceCache, GitStoreContext, PrepareScriptOptions, RunningPnpm,
};
use pnpm_injected_deps_syncer::{CopyFiles, DirPatcher, PublishSource, publish_edits};
use pnpm_patching::{PkgFilesForDiff, prepare_pkg_files_for_diff};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::StoreDir;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};
use tar::Archive;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
enum MaterializedEntry {
    File(String),
    Link(PathBuf),
}

fn write_file(root: &Path, rel: &str, contents: &str) {
    let full = root.join(rel);
    fs::create_dir_all(full.parent().expect("parent dir")).expect("create parent dir");
    fs::write(full, contents).expect("write file");
}

fn create_fixture_workspace() -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(
        "deployAllFiles: false\ndedupeInjectedDeps: false\nsyncInjectedDepsAfterScripts:\n  - build\n",
    );
    write_file(
        &fixture.workspace,
        "package.json",
        &json!({ "name": "root", "private": true }).to_string(),
    );
    write_file(
        &fixture.workspace,
        "packages/app/package.json",
        &json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*" },
            "dependenciesMeta": { "lib": { "injected": true } },
        })
        .to_string(),
    );

    let lib = fixture.workspace.join("packages/lib");
    write_file(
        &lib,
        "package.json",
        &json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["dist", "index.js", "links"],
            "main": "dist/main.js",
            "bin": { "lib-cli": "bin/cli.js" },
            "scripts": {
                "build": r#"node -e "require('fs').writeFileSync('dist/main.js', 'module.exports = 1\n')""#
            }
        })
        .to_string(),
    );
    write_file(&lib, "index.js", "module.exports = 'root'\n");
    write_file(&lib, "README.md", "# lib\n");
    write_file(&lib, "LICENSE", "MIT\n");
    write_file(&lib, "bin/cli.js", "#!/usr/bin/env node\nconsole.log('cli')\n");
    write_file(&lib, "dist/main.js", "module.exports = 1\n");
    write_file(&lib, "dist/keep.js", "module.exports = 'keep'\n");
    write_file(&lib, "dist/ignored.test.js", "test()\n");
    write_file(&lib, "dist/.npmignore", "*.test.js\n");
    write_file(&lib, "dist/sub/nested.js", "module.exports = 'nested'\n");
    write_file(&lib, "src/unpacked.js", "module.exports = 'unpacked'\n");
    write_file(&lib, "unpacked-dir/extra.js", "module.exports = 'extra'\n");
    write_file(&lib, ".DS_Store", "junk");
    write_file(&lib, "foo.orig", "junk");
    write_file(&lib, "npm-debug.log", "junk");

    let outside = fixture.workspace.join("outside-secret.txt");
    fs::write(&outside, "secret\n").expect("write outside file");

    let links_dir = lib.join("links");
    fs::create_dir_all(&links_dir).expect("create links dir");
    pnpm_fs::symlink_file_or_copy(
        Path::new("../index.js"),
        &lib.join("index.js"),
        &links_dir.join("to-selected-file.js"),
    )
    .expect("create symlink to selected file");
    pnpm_fs::symlink_dir(&lib.join("dist/sub"), &links_dir.join("to-selected-dir"))
        .expect("create symlink to selected dir");
    pnpm_fs::symlink_file_or_copy(
        Path::new("../src/unpacked.js"),
        &lib.join("src/unpacked.js"),
        &links_dir.join("to-unselected-file.js"),
    )
    .expect("create symlink to unselected file");
    pnpm_fs::symlink_dir(&lib.join("unpacked-dir"), &links_dir.join("to-unselected-dir"))
        .expect("create symlink to unselected dir");
    let _ = pnpm_fs::create_symlink(
        Path::new("../../../outside-secret.txt"),
        &links_dir.join("escaping.txt"),
        false,
    );

    fixture
}

fn read_materialized_tree(dir: &Path) -> BTreeMap<String, MaterializedEntry> {
    let mut out = BTreeMap::new();
    for entry in WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "node_modules")
    {
        let entry = entry.expect("walk entry");
        let rel = entry
            .path()
            .strip_prefix(dir)
            .expect("strip prefix")
            .to_string_lossy()
            .replace('\\', "/");
        if pnpm_fs::is_symlink(entry.path()) {
            let target = pnpm_fs::read_symlink_dir(entry.path())
                .map(|link_target| pnpm_fs::relative_link_target(entry.path(), link_target))
                .expect("read symlink target");
            out.insert(rel, MaterializedEntry::Link(target));
        } else if entry.file_type().is_file() {
            let content = fs::read_to_string(entry.path()).expect("read file");
            out.insert(rel, MaterializedEntry::File(content));
        }
    }
    out
}

fn unpack_tarball_entries(tgz_path: &Path) -> BTreeSet<String> {
    let file = fs::File::open(tgz_path).expect("open tarball");
    let mut archive = Archive::new(GzDecoder::new(file));
    archive
        .entries()
        .expect("tar entries")
        .map(|entry| {
            let entry = entry.expect("tar entry");
            let path = entry
                .path()
                .expect("entry path")
                .to_string_lossy()
                .replace('\\', "/");
            path.strip_prefix("package/")
                .unwrap_or(&path)
                .to_string()
        })
        .collect()
}

fn run_git(repo: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("spawn git")
        .success();
    assert!(ok, "git {args:?} failed");
}

async fn fetch_git_paths(selected: &pnpm_fs_packlist::SelectedPackageFiles) -> BTreeSet<String> {
    let git_tmp = tempfile::tempdir().expect("git tempdir");
    let git_repo = git_tmp.path().join("repo");
    fs::create_dir_all(&git_repo).expect("create git repo dir");
    for (rel, src) in &selected.files {
        let dest = git_repo.join(rel);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        if let Ok(meta) = fs::metadata(src)
            && meta.is_file()
        {
            fs::copy(src, dest).unwrap();
        }
    }
    run_git(&git_repo, &["init"]);
    run_git(&git_repo, &["config", "user.email", "test@example.com"]);
    run_git(&git_repo, &["config", "user.name", "Test"]);
    run_git(&git_repo, &["add", "."]);
    run_git(&git_repo, &["commit", "-m", "init"]);
    let commit = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&git_repo)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let store_tmp = tempfile::tempdir().expect("store tempdir");
    let store_dir = StoreDir::from(store_tmp.path().to_path_buf());
    let repo_url = format!("file://{}", git_repo.display());
    let git_output = GitFetcher {
        scripts: PrepareScriptOptions {
            ignore: true,
            unsafe_perm: true,
            user_agent: None,
            shell: None,
            node_execpath: None,
            npm_execpath: None,
            running_pnpm: RunningPnpm::default(),
        },
        source: GitSource {
            cache: &GitSourceCache::default(),
            repo: &repo_url,
            commit: &commit,
            path: None,
            shallow_hosts: &[],
            git_bin: None,
        },
        store: GitStoreContext {
            dir: &store_dir,
            index_writer: None,
            files_index_file: "lib@1.0.0\tbuilt",
        },
        allow_build: &(|_| Some(false)),
        package_id: "lib@1.0.0",
        package_name: "lib",
        requester: ".",
    }
    .run::<SilentReporter>()
    .await
    .expect("GitFetcher::run");
    git_output.cas_paths
        .keys()
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_file_selector_agrees_with_pack() {
    let fixture = create_fixture_workspace();
    let workspace = fixture.workspace.clone();
    let lib = workspace.join("packages/lib");

    // 1. Canonical selector: `pnpm_fs_packlist::select_dir_package_files`
    let selected =
        pnpm_fs_packlist::select_dir_package_files(&lib).expect("select_dir_package_files");
    let expected_packlist_paths: BTreeSet<String> = selected.files.keys().cloned().collect();
    for excluded in ["links/escaping.txt", "dist/ignored.test.js", "src/unpacked.js"] {
        let present = expected_packlist_paths.contains(excluded);
        assert!(!present, "expected {excluded} to be excluded");
    }
    for included in [
        "links/to-selected-file.js",
        "links/to-selected-dir",
        "links/to-unselected-file.js",
        "links/to-unselected-dir",
    ] {
        let present = expected_packlist_paths.contains(included);
        assert!(present, "expected {included} to be included");
    }

    // 2. `pnpm pack` tarball
    let pack_out = fixture.command_at(&lib, ["pack", "--ignore-scripts"]);
    assert_success(&pack_out);
    let tgz_path = lib.join("lib-1.0.0.tgz");
    let packed_tarball_paths = unpack_tarball_entries(&tgz_path);
    fs::remove_file(&tgz_path).expect("remove tarball so later selectors do not see it");
    assert_eq!(
        packed_tarball_paths, expected_packlist_paths,
        "pack tarball vs select_dir_package_files",
    );

    // 3. `DirectoryFetcher` (with preserve_symlinks)
    let dir_fetched = DirectoryFetcher {
        directory: lib.clone(),
        include_only_package_files: true,
        resolve_symlinks: false,
        preserve_symlinks: true,
        allow_path_escape: false,
    }
    .run()
    .expect("DirectoryFetcher::run");
    let dir_fetcher_paths: BTreeSet<String> = dir_fetched.files_map
        .keys()
        .cloned()
        .collect();
    assert_eq!(dir_fetcher_paths, expected_packlist_paths, "DirectoryFetcher vs pack");

    // 4. `patch-commit` (`prepare_pkg_files_for_diff`)
    let PkgFilesForDiff::Temporary(patch_temp_dir) =
        prepare_pkg_files_for_diff(&lib).expect("prepare_pkg_files_for_diff")
    else {
        panic!("expected temporary filtered dir for patch-commit");
    };
    let patch_commit_tree = read_materialized_tree(&patch_temp_dir);
    let _ = fs::remove_dir_all(&patch_temp_dir);
    let patch_commit_paths: BTreeSet<String> = patch_commit_tree
        .keys()
        .cloned()
        .collect();
    assert_eq!(patch_commit_paths, expected_packlist_paths, "patch-commit vs pack");

    // 5. `GitFetcher` (imports file entries and file-symlink targets into CAS)
    let expected_file_only_paths: BTreeSet<String> = expected_packlist_paths
        .iter()
        .filter(|rel| fs::metadata(lib.join(rel)).is_ok_and(|m| m.is_file()))
        .cloned()
        .collect();
    let git_fetched_paths = fetch_git_paths(&selected).await;
    assert_eq!(git_fetched_paths, expected_file_only_paths, "GitFetcher vs pack file entries");

    // 6. Injected install (`import_indexed_dir` under `SymlinkTargets::Imported`)
    assert_success(&fixture.command_at(&workspace, ["install"]));
    let injected_copy = fs::canonicalize(workspace.join("packages/app/node_modules/lib"))
        .expect("resolve injected copy");
    let installed_tree = read_materialized_tree(&injected_copy);

    let mut expected_imported_paths = expected_packlist_paths.clone();
    expected_imported_paths.remove("links/to-unselected-dir");
    let installed_paths: BTreeSet<String> = installed_tree.keys().cloned().collect();
    assert_eq!(installed_paths, expected_imported_paths, "injected install paths vs pack");
    #[cfg(unix)]
    {
        let selected_file_link = installed_tree.get("links/to-selected-file.js");
        let expected_file_link = MaterializedEntry::Link(PathBuf::from("../index.js"));
        assert_eq!(selected_file_link, Some(&expected_file_link));
        let selected_dir_link = installed_tree.get("links/to-selected-dir");
        let expected_dir_link = MaterializedEntry::Link(PathBuf::from("../dist/sub"));
        assert_eq!(selected_dir_link, Some(&expected_dir_link));
    }
    let unselected_file_entry = installed_tree.get("links/to-unselected-file.js");
    let expected_unselected_file =
        MaterializedEntry::File("module.exports = 'unpacked'\n".to_string());
    assert_eq!(unselected_file_entry, Some(&expected_unselected_file));

    // 7. Injected sync (`syncInjectedDepsAfterScripts`, `DirPatcher`, and `publish_edits`)
    assert_success(&fixture.command_at(&lib, ["run", "build"]));
    let synced_after_script_tree = read_materialized_tree(&injected_copy);
    assert_eq!(
        synced_after_script_tree, installed_tree,
        "syncInjectedDepsAfterScripts vs injected install",
    );

    let patcher_target = tempfile::tempdir().expect("patcher target");
    let patchers = DirPatcher::from_multiple_targets(
        &lib,
        &[patcher_target.path().to_path_buf()],
        CopyFiles::PackageFiles,
    )
    .expect("DirPatcher::from_multiple_targets");
    for patcher in &patchers {
        patcher.apply().expect("DirPatcher::apply");
    }
    let patcher_tree = read_materialized_tree(patcher_target.path());
    assert_eq!(patcher_tree, installed_tree, "DirPatcher vs injected install");

    let publish_target = tempfile::tempdir().expect("publish target");
    let publish_source =
        PublishSource::load(&lib, CopyFiles::PackageFiles).expect("PublishSource::load");
    publish_edits(&publish_source, publish_target.path(), SystemTime::UNIX_EPOCH)
        .expect("publish_edits");
    let publish_tree = read_materialized_tree(publish_target.path());
    assert_eq!(publish_tree, installed_tree, "publish_edits vs injected install");

    // 8. `pnpm deploy`
    let deploy_dir = workspace.join("deployed-app");
    assert_success(&fixture.command_at(
        &workspace,
        ["--filter", "app", "deploy", deploy_dir.to_str().unwrap()],
    ));
    let deployed_lib =
        fs::canonicalize(deploy_dir.join("node_modules/lib")).expect("resolve deployed lib");
    let deployed_tree = read_materialized_tree(&deployed_lib);
    assert_eq!(deployed_tree, installed_tree, "deploy node_modules/lib vs injected install");
}
