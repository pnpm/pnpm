//! Ports the ref-resolution helpers from pnpm's
//! [`index.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/index.ts#L116-L200):
//! `resolveRef` / `getRepoRefs` / `resolveRefFromRefs` / `resolveVTags`,
//! plus the [`GitCommandRunner`] capability seam the production runner
//! plugs into.

use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    pin::Pin,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};

/// Capability seam for `git ls-remote`.
///
/// Real installs supply an impl that shells out to the system `git`
/// binary via `tokio::process::Command`; tests supply a fake that
/// returns canned stdout for a given (repo, args) pair.
pub trait GitCommandRunner: Send + Sync {
    /// Invoke `git ls-remote <repo> [<ref> <ref>^{}]` (or
    /// `git ls-remote <repo>` when `ref_` is `None`) and return the
    /// captured stdout on success. Match upstream's `graceful-git`
    /// retry-of-one behaviour (one attempt + one retry, total two
    /// attempts at most).
    fn ls_remote<'a>(
        &'a self,
        repo: &'a str,
        ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>>;
}

/// Error from a [`GitCommandRunner::ls_remote`] invocation. Returned
/// verbatim through [`GitResolveRefError::Runner`].
#[derive(Debug, Display, Error, Diagnostic)]
#[display("git ls-remote failed: {message}")]
#[diagnostic(code(ERR_PNPM_GIT_LS_REMOTE_FAILED))]
pub struct GitRunError {
    pub message: String,
}

/// Errors raised by [`resolve_ref`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum GitResolveRefError {
    /// `git ls-remote` failed.
    #[display("{_0}")]
    Runner(#[error(source)] GitRunError),

    /// Mirrors upstream's `ERR_PNPM_GIT_AMBIGUOUS_REF`. Raised when a
    /// partial commit reference resolves to a commit whose hash does
    /// not start with the reference (the resolver picked a branch /
    /// tag whose tip happened to match the prefix, which is the
    /// scenario the original `PnpmError` was added for).
    #[display("resolved commit {commit} from commit-ish reference {ref_}")]
    #[diagnostic(code(ERR_PNPM_GIT_AMBIGUOUS_REF))]
    AmbiguousRef {
        #[error(not(source))]
        ref_: String,
        #[error(not(source))]
        commit: String,
    },

    /// Mirrors upstream's plain `Could not resolve <ref> to a commit
    /// of <repo>.` error.
    #[display("Could not resolve {ref_} to a commit of {repo}.")]
    UnknownRef {
        #[error(not(source))]
        ref_: String,
        #[error(not(source))]
        repo: String,
    },

    /// Mirrors upstream's `Could not resolve <range> to a commit of
    /// <repo>. Available versions are: <v1>, <v2>` error.
    #[display(
        "Could not resolve {range} to a commit of {repo}. Available versions are: {available}"
    )]
    UnknownRange {
        #[error(not(source))]
        range: String,
        #[error(not(source))]
        repo: String,
        #[error(not(source))]
        available: String,
    },
}

/// Pin a git reference to a commit SHA.
///
/// Mirrors upstream's
/// [`resolveRef`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/index.ts#L138-L149).
///
/// * Full 40-char hex commit → return as-is, no network round-trip.
/// * Partial hex commit (7-40 chars, no range) → query `ls-remote`
///   with no ref filter, then search ref tips for a single matching
///   prefix. Surface [`GitResolveRefError::AmbiguousRef`] when the
///   matched commit does not start with the partial hash.
/// * Branch / tag (no range) → query `ls-remote <ref> <ref>^{}` and
///   look up the resolved SHA in a fixed precedence order.
/// * Semver range (`#semver:<range>`) → query `ls-remote` with no
///   ref filter, filter tags to those matching upstream's
///   `^refs/tags/v?\d+\.\d+\.\d+(?:[-+].+)?(?:\^\{\})?$` shape, run
///   `maxSatisfying`, look up the chosen tag.
pub async fn resolve_ref<Runner: GitCommandRunner + ?Sized>(
    runner: &Runner,
    repo: &str,
    ref_: &str,
    range: Option<&str>,
) -> Result<String, GitResolveRefError> {
    let committish = is_committish(ref_);
    if committish && ref_.len() == 40 {
        return Ok(ref_.to_string());
    }
    // Upstream passes `null` for `ref` when either `range` is set or
    // the ref looks like a committish (we don't have a single
    // canonical ref name to filter on). Mirror that.
    let filter = if range.is_some() || committish { None } else { Some(ref_) };
    let stdout = runner.ls_remote(repo, filter).await.map_err(GitResolveRefError::Runner)?;
    let refs = parse_ls_remote(&stdout);
    let commit = resolve_ref_from_refs(&refs, repo, ref_, committish, range)?;
    if committish && !commit.starts_with(ref_) {
        return Err(GitResolveRefError::AmbiguousRef { ref_: ref_.to_string(), commit });
    }
    Ok(commit)
}

/// `true` when `ref` is a 7-40-character lowercase hex string.
/// Mirrors upstream's `ref.match(/^[0-9a-f]{7,40}$/)`.
fn is_committish(ref_: &str) -> bool {
    let bytes = ref_.as_bytes();
    bytes.len() >= 7
        && bytes.len() <= 40
        && bytes.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Parse the `git ls-remote` stdout into `{ ref_name -> commit_sha }`.
fn parse_ls_remote(stdout: &str) -> HashMap<String, String> {
    let mut refs = HashMap::new();
    for line in stdout.split('\n') {
        if line.is_empty() {
            continue;
        }
        if let Some((commit, ref_name)) = line.split_once('\t') {
            refs.insert(ref_name.to_string(), commit.to_string());
        }
    }
    refs
}

fn resolve_ref_from_refs(
    refs: &HashMap<String, String>,
    repo: &str,
    ref_: &str,
    committish: bool,
    range: Option<&str>,
) -> Result<String, GitResolveRefError> {
    let Some(range) = range else {
        // Exact-ref lookup order matches upstream verbatim.
        let lookup_keys = [
            ref_.to_string(),
            format!("refs/{ref_}"),
            format!("refs/tags/{ref_}^{{}}"),
            format!("refs/tags/{ref_}"),
            format!("refs/heads/{ref_}"),
        ];
        for key in &lookup_keys {
            if let Some(commit) = refs.get(key) {
                return Ok(commit.clone());
            }
        }
        if committish {
            // Partial-commit fallback: any ref tip starting with the
            // partial commit string. Dedupe across multiple refs that
            // point at the same commit (`refs/heads/main` and
            // `refs/tags/v1` may both point at the same SHA).
            let mut matches = BTreeSet::new();
            for value in refs.values() {
                if value.starts_with(ref_) {
                    matches.insert(value.clone());
                }
            }
            if matches.len() == 1 {
                return Ok(matches.into_iter().next().unwrap());
            }
        }
        return Err(GitResolveRefError::UnknownRef {
            ref_: ref_.to_string(),
            repo: repo.to_string(),
        });
    };

    // Semver range: walk tag refs, keep the ones matching upstream's
    // v?<n.n.n>(-...|+...)? regex, dedupe, semver-sort, return the max
    // satisfying.
    let mut v_tags: BTreeSet<String> = BTreeSet::new();
    for key in refs.keys() {
        if !looks_like_version_tag(key) {
            continue;
        }
        let cleaned = key
            .strip_prefix("refs/tags/")
            .expect("guard above ensures the prefix")
            .strip_suffix("^{}")
            .unwrap_or_else(|| key.strip_prefix("refs/tags/").expect("guarded"));
        if Version::parse(cleaned).is_ok() || Version::parse(strip_v(cleaned)).is_ok() {
            v_tags.insert(cleaned.to_string());
        }
    }

    let parsed_range = Range::parse(range).map_err(|_| GitResolveRefError::UnknownRange {
        range: range.to_string(),
        repo: repo.to_string(),
        available: v_tags.iter().cloned().collect::<Vec<_>>().join(", "),
    })?;
    let pick = resolve_v_tags(&v_tags, &parsed_range);
    if let Some(tag) = pick {
        let commit = refs
            .get(&format!("refs/tags/{tag}^{{}}"))
            .or_else(|| refs.get(&format!("refs/tags/{tag}")))
            .cloned();
        if let Some(commit) = commit {
            return Ok(commit);
        }
    }
    Err(GitResolveRefError::UnknownRange {
        range: range.to_string(),
        repo: repo.to_string(),
        available: v_tags.iter().cloned().collect::<Vec<_>>().join(", "),
    })
}

fn strip_v(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// `true` when `key` matches the upstream `refs/tags/v?<n.n.n>(...)?
/// (^\{\})?` regex.
fn looks_like_version_tag(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("refs/tags/") else { return false };
    let rest = rest.strip_suffix("^{}").unwrap_or(rest);
    let rest = strip_v(rest);
    // Must start with `\d+\.\d+\.\d+`. The semver parser is lenient
    // about trailing prerelease/build content, so we only need to
    // gate the numeric prefix.
    let mut chars = rest.chars().peekable();
    for _ in 0..3 {
        let mut saw_digit = false;
        while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
            chars.next();
            saw_digit = true;
        }
        if !saw_digit {
            return false;
        }
        // Between the three groups we expect a `.`; after the third
        // group anything (or nothing) goes.
        if chars.peek() == Some(&'.') {
            chars.next();
        } else {
            // ok only if we've consumed all three groups
        }
    }
    true
}

/// Return the highest tag in `tags` that satisfies `range`. Mirrors
/// upstream's `semver.maxSatisfying(vTags, range, /* loose */ true)`.
fn resolve_v_tags(tags: &BTreeSet<String>, range: &Range) -> Option<String> {
    let mut best: Option<(Version, String)> = None;
    for tag in tags {
        let parsed = Version::parse(tag).or_else(|_| Version::parse(strip_v(tag))).ok()?;
        if range.satisfies(&parsed) {
            match best {
                Some((ref best_v, _)) if best_v >= &parsed => {}
                _ => best = Some((parsed.clone(), tag.clone())),
            }
        }
    }
    best.map(|(_, tag)| tag)
}

#[cfg(test)]
mod tests {
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
        let commit =
            resolve_ref(&stub, "https://example.com/repo.git", "canary", None).await.unwrap();
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
        // Single ref tip starts with `deadbe` but not `deadbf` →
        // resolves to the matching commit, then trips the
        // partial-prefix sanity check (matched commit does not start
        // with `deadbf`).
        let stub = stub("deadbeef1234567890123456789012345678abcd\trefs/heads/x\n");
        let err = resolve_ref(&stub, "repo", "deadbf12", None).await.expect_err("ambig");
        // First the lookup falls through (no exact ref match), then
        // partial-commit search finds zero matches → UnknownRef.
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
}
