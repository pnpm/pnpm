//! Ref-resolution helpers: [`resolve_ref`] / [`parse_ls_remote`] /
//! [`resolve_ref_from_refs`] / [`resolve_v_tags`], plus the
//! [`GitCommandRunner`] capability seam the production runner plugs
//! into.

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
    /// captured stdout on success. Uses a retry-of-one policy (one
    /// attempt + one retry, total two attempts at most).
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

    /// `ERR_PNPM_GIT_AMBIGUOUS_REF`. Raised when a partial commit
    /// reference resolves to a commit whose hash does not start with
    /// the reference (the resolver picked a branch / tag whose tip
    /// happened to match the prefix).
    #[display("resolved commit {commit} from commit-ish reference {ref_}")]
    #[diagnostic(code(ERR_PNPM_GIT_AMBIGUOUS_REF))]
    AmbiguousRef {
        #[error(not(source))]
        ref_: String,
        #[error(not(source))]
        commit: String,
    },

    /// Plain `Could not resolve <ref> to a commit of <repo>.` error.
    #[display("Could not resolve {ref_} to a commit of {repo}.")]
    UnknownRef {
        #[error(not(source))]
        ref_: String,
        #[error(not(source))]
        repo: String,
    },

    /// `Could not resolve <range> to a commit of <repo>. Available
    /// versions are: <v1>, <v2>` error.
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
    // Pass `None` for the ref filter when either `range` is set or the
    // ref looks like a committish: there is no single canonical ref
    // name to filter on in those cases.
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
        // Exact-ref lookup, in priority order.
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

    // Semver range: walk tag refs, keep the ones shaped like
    // v?<n.n.n>(-...|+...)?, dedupe, semver-sort, return the max
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

/// `true` when `key` is shaped like `refs/tags/v?<n.n.n>(...)?(^\{\})?`.
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

/// Return the highest tag in `tags` that satisfies `range`, parsing
/// tags leniently (with or without a leading `v`).
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
mod tests;
