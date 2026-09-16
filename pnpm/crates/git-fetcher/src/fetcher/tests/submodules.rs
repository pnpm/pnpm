use super::exec_git;
use crate::{checkout_submodules, fetcher::submodule_protocols};
use pnpm_testing_utils::git_repo::GitRepoFixture;
use std::ffi::OsStr;

#[test]
fn helper_transports_are_rejected_even_when_git_configuration_allows_them() {
    let root = tempfile::tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "unsafe");
    repo.write_file("README", "fixture");
    let commit = repo.commit("init");
    let work = root.path().join("unsafe-src");
    repo.write_file(
        ".gitmodules",
        "[submodule \"native\"]\npath = native\nurl = ext::sh -c touch% marker\n",
    );
    for args in [
        vec!["update-index", "--add", "--cacheinfo", "160000", &commit, "native"],
        vec!["config", "protocol.ext.allow", "always"],
    ] {
        exec_git(&args, Some(&work)).unwrap();
    }
    let error = checkout_submodules(&work).unwrap_err();
    eprintln!("Unsafe submodule transport must fail: {error:?}");
    assert!(error.to_string().contains("transport 'ext' not allowed"));
    assert!(!work.join("marker").exists());
}

#[test]
fn inherited_protocol_allowlists_are_only_narrowed() {
    for (inherited, expected) in [
        (None, "file:git:http:https:ssh"),
        (Some("https"), "https"),
        (Some("https:ext"), "https"),
        (Some("ssh:file"), "file:ssh"),
        (Some(""), ""),
        (Some("ext"), ""),
    ] {
        assert_eq!(submodule_protocols(inherited.map(OsStr::new)), expected);
    }
}
