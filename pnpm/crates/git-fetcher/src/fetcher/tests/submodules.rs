use super::exec_git;
use crate::checkout_submodules;
use pnpm_testing_utils::git_repo::GitRepoFixture;
use std::path::{Path, PathBuf};

fn submodule_checkout(root: &Path, url: &str) -> PathBuf {
    let repo = GitRepoFixture::init(root, "parent");
    repo.write_file("README", "fixture");
    let commit = repo.commit("init");
    let work = root.join("parent-src");
    repo.write_file(
        ".gitmodules",
        &format!("[submodule \"native\"]\npath = native\nurl = {url}\n"),
    );
    exec_git(&["update-index", "--add", "--cacheinfo", "160000", &commit, "native"], Some(&work))
        .unwrap();
    work
}

#[test]
fn helper_transports_are_rejected_even_when_git_configuration_allows_them() {
    let root = tempfile::tempdir().unwrap();
    let work = submodule_checkout(root.path(), "ext::sh -c touch% marker");
    exec_git(&["config", "protocol.ext.allow", "always"], Some(&work)).unwrap();
    let error = checkout_submodules(&work).unwrap_err();
    eprintln!("Unsafe submodule transport must fail: {error:?}");
    assert!(
        error
            .to_string()
            .contains("transport 'ext' not allowed")
    );
    assert!(!work.join("marker").exists());
}

#[test]
fn configured_protocol_bans_are_preserved_for_submodules() {
    for (protocol, url) in
        [("file", "file:///missing/repository"), ("ssh", "ssh://invalid.example/repository")]
    {
        let root = tempfile::tempdir().unwrap();
        let work = submodule_checkout(root.path(), url);
        exec_git(&["config", &format!("protocol.{protocol}.allow"), "never"], Some(&work)).unwrap();
        let error = checkout_submodules(&work).unwrap_err();
        eprintln!("Configured {protocol} bans must be enforced: {error:?}");
        assert!(
            error
                .to_string()
                .contains(&format!("transport '{protocol}' not allowed")),
        );
    }
}
