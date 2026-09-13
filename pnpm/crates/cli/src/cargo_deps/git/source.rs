use super::{GitReference, Path, PathBuf, Result};
use miette::{IntoDiagnostic, WrapErr};

pub(super) fn git_reference(source: &cargo_lock::SourceId) -> Option<(&'static str, String)> {
    match source.git_reference() {
        Some(GitReference::Branch(branch)) => Some(("branch", branch.clone())),
        Some(GitReference::Tag(tag)) => Some(("tag", tag.clone())),
        Some(GitReference::Rev(rev)) => Some(("rev", rev.clone())),
        Some(GitReference::DefaultBranch) | None => None,
    }
}

pub(super) fn canonical_checkout_root(checkout: &Path, repository: &str) -> Result<PathBuf> {
    dunce::canonicalize(checkout)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the checkout of {repository}"))
}
