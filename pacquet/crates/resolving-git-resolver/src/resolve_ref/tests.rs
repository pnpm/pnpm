use super::{
    GitCommandRunner, GitResolveRefError, GitRunError, looks_like_version_tag, parse_ls_remote,
    resolve_ref,
};
use std::{future::Future, pin::Pin, sync::Mutex};

struct Stub {
    result: Result<String, String>,
    last_args: Mutex<Vec<(String, Option<String>)>>,
}
impl GitCommandRunner for Stub {
    fn ls_remote<'a>(
        &'a self,
        repo: &'a str,
        ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        self.last_args.lock().unwrap().push((repo.to_string(), ref_.map(str::to_string)));
        Box::pin(async move { self.result.clone().map_err(|message| GitRunError { message }) })
    }
}
fn stub(stdout: &str) -> Stub {
    Stub { result: Ok(stdout.to_string()), last_args: Mutex::new(Vec::new()) }
}

#[tokio::test]
async fn full_commit_returns_unchanged_without_network() {
    let stub = stub("");
    let commit = resolve_ref(
        &stub,
        "https://example.com/repo.git",
        "163360a8d3ae6bee9524541043197ff356f8ed99",
        None,
    )
    .await
    .expect("resolved");
    assert_eq!(commit, "163360a8d3ae6bee9524541043197ff356f8ed99");
    assert!(stub.last_args.lock().unwrap().is_empty(), "no ls-remote for full commit");
}

#[tokio::test]
async fn branch_lookup_uses_refs_heads() {
    let stub = stub("4c39fbc124cd4944ee51cb082ad49320fab58121\trefs/heads/canary\n");
    let commit = resolve_ref(&stub, "https://example.com/repo.git", "canary", None).await.unwrap();
    assert_eq!(commit, "4c39fbc124cd4944ee51cb082ad49320fab58121");
}

#[tokio::test]
async fn annotated_tag_prefers_dereferenced_commit() {
    let stub = stub(concat!(
        "deadbeef00000000000000000000000000000000\trefs/tags/v1.0.0\n",
        "6dcce91c268805d456b8a575b67d7febc7ae2933\trefs/tags/v1.0.0^{}\n",
    ));
    let commit = resolve_ref(&stub, "repo", "v1.0.0", None).await.unwrap();
    assert_eq!(commit, "6dcce91c268805d456b8a575b67d7febc7ae2933");
}

#[tokio::test]
async fn partial_commit_ambiguous_branch_raises() {
    let stub = stub("0000000000000000000000000000000000000000\trefs/heads/main\n");
    let err = resolve_ref(&stub, "repo", "deadbeef", None).await.expect_err("ambiguous");
    match err {
        GitResolveRefError::UnknownRef { .. } => {}
        other => panic!("expected UnknownRef, got {other:?}"),
    }
}

#[tokio::test]
async fn partial_commit_matches_single_ref() {
    let stub = stub("deadbeef1234567890123456789012345678abcd\trefs/heads/feat\n");
    let commit = resolve_ref(&stub, "repo", "deadbeef", None).await.unwrap();
    assert_eq!(commit, "deadbeef1234567890123456789012345678abcd");
}

#[tokio::test]
async fn ambiguous_partial_commit_mismatch_errors() {
    let stub = stub("deadbeef1234567890123456789012345678abcd\trefs/heads/x\n");
    let err = resolve_ref(&stub, "repo", "deadbf12", None).await.expect_err("ambig");
    assert!(matches!(err, GitResolveRefError::UnknownRef { .. }));
}

#[tokio::test]
async fn semver_range_picks_max_satisfying() {
    let stub = stub(concat!(
        "0000000000000000000000000000000000000000\tHEAD\n",
        "ed3de20970d980cf21a07fd8b8732c70d5182303\trefs/tags/v0.0.38\n",
        "cba04669e621b85fbdb33371604de1a2898e68e9\trefs/tags/v0.0.39\n",
    ));
    let commit = resolve_ref(&stub, "repo", "HEAD", Some("~0.0.38")).await.unwrap();
    assert_eq!(commit, "cba04669e621b85fbdb33371604de1a2898e68e9");
}

#[tokio::test]
async fn semver_no_match_lists_available_versions() {
    let stub = stub(concat!(
        "aaaa\trefs/tags/v1.0.0\n",
        "bbbb\trefs/tags/v1.0.1\n",
        "cccc\trefs/tags/v2.0.0\n",
    ));
    let err = resolve_ref(&stub, "repo", "HEAD", Some("^100.0.0")).await.expect_err("err");
    match err {
        GitResolveRefError::UnknownRange { available, .. } => {
            assert!(available.contains("v1.0.0"));
            assert!(available.contains("v2.0.0"));
        }
        other => panic!("expected UnknownRange, got {other:?}"),
    }
}

#[test]
fn version_tag_regex() {
    assert!(looks_like_version_tag("refs/tags/1.0.0"));
    assert!(looks_like_version_tag("refs/tags/v1.0.0"));
    assert!(looks_like_version_tag("refs/tags/v1.0.0-beta.1"));
    assert!(looks_like_version_tag("refs/tags/1.0.0^{}"));
    assert!(!looks_like_version_tag("refs/tags/release"));
    assert!(!looks_like_version_tag("refs/heads/main"));
}

#[test]
fn parse_ls_remote_ignores_blank_lines() {
    let refs = parse_ls_remote("abc\trefs/heads/main\n\n");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs.get("refs/heads/main").map(String::as_str), Some("abc"));
}
