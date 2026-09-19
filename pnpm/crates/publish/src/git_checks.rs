//! The git precondition checks `pnpm publish` runs: refuse to publish from an
//! unclean tree, the wrong branch, or behind the remote.

use std::path::Path;

use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_git_utils::{
    get_current_branch, is_git_repo, is_head_detached, is_remote_history_clean,
    is_working_tree_clean,
};

use crate::capabilities::{ConfirmPrompt, RunCommand};

const GIT_CHECKS_HINT: &str = r#"If you want to disable Git checks on publish, set the "git-checks" setting to "false", or run again with "--no-git-checks"."#;

/// Run the publish git checks for `cwd`. A no-op when `git_checks_enabled` is
/// false or `cwd` is not a git repository. A detached HEAD is allowed in CI
/// after checking that the working tree is clean.
pub fn run_git_checks<Sys>(
    cwd: &Path,
    git_checks_enabled: bool,
    publish_branch: Option<&str>,
    ci: bool,
) -> Result<(), GitCheckError>
where
    Sys: RunCommand + ConfirmPrompt,
{
    if !git_checks_enabled || !is_git_repo::<Sys>(cwd) {
        return Ok(());
    }

    if !is_working_tree_clean::<Sys>(cwd) {
        return Err(GitCheckError::Unclean);
    }

    let branches: Vec<String> = match publish_branch {
        Some(branch) => vec![branch.to_owned()],
        None => vec!["master".to_owned(), "main".to_owned()],
    };
    let current_branch = match get_current_branch::<Sys>(cwd) {
        Some(branch) => branch,
        None if ci && is_head_detached::<Sys>(cwd) => return Ok(()),
        None => return Err(GitCheckError::UnknownBranch { branches: branches.join("|") }),
    };

    check_publish_branch::<Sys>(&current_branch, &branches)?;

    if !is_remote_history_clean::<Sys>(cwd) {
        return Err(GitCheckError::NotLatest);
    }

    Ok(())
}

fn check_publish_branch<Sys: ConfirmPrompt>(
    current_branch: &str,
    branches: &[String],
) -> Result<(), GitCheckError> {
    if branches
        .iter()
        .any(|branch| branch == current_branch)
    {
        return Ok(());
    }
    let branches_display = branches.join("|");
    let message = format!(
        r#"You're on branch "{current_branch}" but your "publish-branch" is set to "{branches_display}". Do you want to continue?"#,
    );
    if !Sys::confirm(&message) {
        return Err(GitCheckError::NotCorrectBranch { branches: branches_display });
    }
    Ok(())
}

/// The git working-tree precondition that failed. Each variant is an
/// `ERR_PNPM_GIT_*` publish error carrying the same disable-checks hint.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum GitCheckError {
    #[display("Unclean working tree. Commit or stash changes first.")]
    #[diagnostic(code(ERR_PNPM_GIT_UNCLEAN), help("{GIT_CHECKS_HINT}"))]
    Unclean,

    #[display(
        "The Git HEAD may not attached to any branch, but your \"publish-branch\" is set to \"{branches}\"."
    )]
    #[diagnostic(code(ERR_PNPM_GIT_UNKNOWN_BRANCH), help("{GIT_CHECKS_HINT}"))]
    UnknownBranch { branches: String },

    #[display("Branch is not on '{branches}'.")]
    #[diagnostic(code(ERR_PNPM_GIT_NOT_CORRECT_BRANCH), help("{GIT_CHECKS_HINT}"))]
    NotCorrectBranch { branches: String },

    #[display("Remote history differs. Please pull changes.")]
    #[diagnostic(code(ERR_PNPM_GIT_NOT_LATEST), help("{GIT_CHECKS_HINT}"))]
    NotLatest,
}

#[cfg(test)]
mod tests;
