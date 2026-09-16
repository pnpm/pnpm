use super::exec_git;
use crate::{checkout_submodules, fetcher::submodule_protocols};
use pnpm_testing_utils::git_repo::GitRepoFixture;
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

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
    assert!(error.to_string().contains("transport 'ext' not allowed"));
    assert!(!work.join("marker").exists());
}

#[test]
fn inherited_protocol_allowlists_are_only_narrowed() {
    let policies = HashMap::from([("protocol.file.allow".to_string(), "always".to_string())]);
    for (inherited, expected) in [
        (None, "file:git:http:https:ssh"),
        (Some("https"), "https"),
        (Some("https:ext"), "https"),
        (Some("ssh:file"), "file:ssh"),
        (Some(""), ""),
        (Some("ext"), ""),
    ] {
        assert_eq!(submodule_protocols(inherited.map(OsStr::new), &policies), expected);
    }
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

#[test]
fn recursive_user_protocol_policies_remain_restricted() {
    let policies = HashMap::new();
    assert_eq!(submodule_protocols(None, &policies), "git:http:https:ssh");
    for policy in ["never", "user", "NEVER", "User"] {
        let policies = HashMap::from([("protocol.allow".to_string(), policy.to_string())]);
        assert_eq!(submodule_protocols(None, &policies), "");
    }
}

#[test]
fn protocol_policy_values_are_case_insensitive() {
    for policy in ["always", "ALWAYS", "Always"] {
        let policies = HashMap::from([("protocol.allow".to_string(), policy.to_string())]);
        assert_eq!(submodule_protocols(None, &policies), "file:git:http:https:ssh");
        let policies = HashMap::from([
            ("protocol.allow".to_string(), "never".to_string()),
            ("protocol.https.allow".to_string(), policy.to_string()),
        ]);
        assert_eq!(submodule_protocols(None, &policies), "https");
    }
}
