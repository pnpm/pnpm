//! Port of `@pnpm/network.git-utils` plus the git-check orchestration at the
//! top of `publish.ts`: refuse to publish from an unclean tree, the wrong
//! branch, or behind the remote.

use std::{fs, path::Path};

use pacquet_diagnostics::miette::{self, Diagnostic};

use crate::capabilities::{ConfirmPrompt, RunCommand};

const GIT_CHECKS_HINT: &str = r#"If you want to disable Git checks on publish, set the "git-checks" setting to "false", or run again with "--no-git-checks"."#;

/// Run the publish git checks for `cwd`. A no-op when `git_checks_enabled` is
/// false or `cwd` is not a git repository. Ports the git-check block of TS
/// `publish`.
pub fn run_git_checks<Sys>(
    cwd: &Path,
    git_checks_enabled: bool,
    publish_branch: Option<&str>,
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
    let branches_display = branches.join("|");

    let Some(current_branch) = get_current_branch::<Sys>(cwd) else {
        return Err(GitCheckError::UnknownBranch { branches: branches_display });
    };

    if !branches.contains(&current_branch) {
        let message = format!(
            "You're on branch \"{current_branch}\" but your \"publish-branch\" is set to \"{branches_display}\". Do you want to continue?",
        );
        if !Sys::confirm(&message) {
            return Err(GitCheckError::NotCorrectBranch { branches: branches_display });
        }
    }

    if !is_remote_history_clean::<Sys>(cwd) {
        return Err(GitCheckError::NotLatest);
    }

    Ok(())
}

/// Whether `cwd` is inside a git repository. Ports `isGitRepo`.
#[must_use]
pub fn is_git_repo<Sys: RunCommand>(cwd: &Path) -> bool {
    git_ok::<Sys>(&["rev-parse", "--git-dir"], cwd)
}

/// Whether the working tree has no uncommitted changes. Ports
/// `isWorkingTreeClean`.
#[must_use]
pub fn is_working_tree_clean<Sys: RunCommand>(cwd: &Path) -> bool {
    match Sys::run("git", &["status", "--porcelain"], Some(cwd)) {
        Ok(output) if output.success => output.stdout.is_empty(),
        _ => false,
    }
}

/// Whether the local branch is not behind its upstream. Ports
/// `isRemoteHistoryClean` (a missing upstream is treated as clean).
#[must_use]
pub fn is_remote_history_clean<Sys: RunCommand>(cwd: &Path) -> bool {
    match Sys::run("git", &["rev-list", "--count", "--left-only", "@{u}...HEAD"], Some(cwd)) {
        Ok(output) if output.success => {
            output.stdout.trim() == "0" || output.stdout.trim().is_empty()
        }
        _ => true,
    }
}

/// The current branch name, or `None` when HEAD is detached. Ports
/// `getCurrentBranch`: reads `.git/HEAD` first, then falls back to
/// `git symbolic-ref`.
#[must_use]
pub fn get_current_branch<Sys: RunCommand>(cwd: &Path) -> Option<String> {
    match read_branch_from_head_file(cwd) {
        HeadBranch::Branch(branch) => Some(branch),
        HeadBranch::Detached => None,
        HeadBranch::Unknown => {
            match Sys::run("git", &["symbolic-ref", "--short", "HEAD"], Some(cwd)) {
                Ok(output) if output.success => Some(output.stdout.trim().to_owned()),
                _ => None,
            }
        }
    }
}

/// The three outcomes of reading `.git/HEAD`: a branch name, a detached HEAD,
/// or "could not determine — fall back to `git symbolic-ref`". Mirrors the TS
/// `string | null | undefined` return of `readBranchFromHeadFile`.
enum HeadBranch {
    Branch(String),
    Detached,
    Unknown,
}

/// Read the branch name from `.git/HEAD` without spawning git. Ports
/// `readBranchFromHeadFile`, including the worktree/submodule `.git` file
/// indirection.
fn read_branch_from_head_file(cwd: &Path) -> HeadBranch {
    let dot_git = cwd.join(".git");
    let Ok(metadata) = fs::symlink_metadata(&dot_git) else {
        return HeadBranch::Unknown;
    };
    let git_dir = if metadata.is_dir() {
        dot_git
    } else if metadata.is_file() {
        let Ok(content) = fs::read_to_string(&dot_git) else {
            return HeadBranch::Unknown;
        };
        match content.trim().strip_prefix("gitdir:").map(str::trim) {
            Some(path) if Path::new(path).is_absolute() => Path::new(path).to_path_buf(),
            Some(path) => cwd.join(path),
            None => return HeadBranch::Unknown,
        }
    } else {
        return HeadBranch::Unknown;
    };

    match fs::read_to_string(git_dir.join("HEAD")) {
        Ok(head) => match head.trim().strip_prefix("ref:").map(str::trim) {
            Some(reference) => match reference.strip_prefix("refs/heads/") {
                Some(branch) => HeadBranch::Branch(branch.to_owned()),
                None => HeadBranch::Detached,
            },
            None => HeadBranch::Detached,
        },
        Err(_) => HeadBranch::Unknown,
    }
}

fn git_ok<Sys: RunCommand>(args: &[&str], cwd: &Path) -> bool {
    Sys::run("git", args, Some(cwd)).is_ok_and(|output| output.success)
}

/// The git working-tree precondition that failed. Ports pnpm's `GIT_*`
/// publish errors; each carries the same disable-checks hint.
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
