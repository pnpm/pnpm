use super::{CommandOutput, RunCommand, get_current_branch, is_head_detached};
use std::{fs, io, path::Path};
use tempfile::TempDir;

// The real provider is only reached by the FIFO tests, which need a
// filesystem object Windows has no equivalent of.
#[cfg(unix)]
use super::Host;

/// A provider whose subprocess spawn is a hard error, so a test that
/// reaches it fails instead of consulting the host's real repository.
struct NoGit;

impl RunCommand for NoGit {
    fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
        unreachable!("the branch is readable from .git/HEAD without spawning git")
    }
}

/// A provider whose `git symbolic-ref` fails, which is what a repository
/// the `.git/HEAD` read declined to answer for looks like.
struct GitFails;

impl RunCommand for GitFails {
    fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
        Ok(CommandOutput { success: false, stdout: String::new(), stderr: String::new() })
    }
}

fn repo_with_head(head: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/HEAD"), head).unwrap();
    dir
}

#[test]
fn reads_the_branch_from_the_head_file() {
    let repo = repo_with_head("ref: refs/heads/feature/a-b\n");
    assert_eq!(get_current_branch::<NoGit>(repo.path()).as_deref(), Some("feature/a-b"));
}

#[test]
fn a_detached_head_has_no_branch() {
    let repo = repo_with_head("0123456789abcdef0123456789abcdef01234567\n");
    assert_eq!(get_current_branch::<NoGit>(repo.path()), None);
}

/// A linked worktree's `.git` is a file pointing at the real git dir;
/// the branch lives in the `HEAD` of the directory it names.
#[test]
fn follows_the_gitdir_indirection_of_a_worktree() {
    let dir = TempDir::new().unwrap();
    let git_dir = dir.path().join("real-git-dir");
    fs::create_dir(&git_dir).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/linked\n").unwrap();
    let worktree = dir.path().join("worktree");
    fs::create_dir(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: ../real-git-dir\n").unwrap();

    assert_eq!(get_current_branch::<NoGit>(&worktree).as_deref(), Some("linked"));
}

/// Without a readable `.git/HEAD` the answer comes from `git
/// symbolic-ref`, whose failure (not a repository) is `None`.
#[test]
fn falls_back_to_the_git_subprocess() {
    struct GitSaysMain;
    impl RunCommand for GitSaysMain {
        fn run(program: &str, args: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            assert_eq!(program, "git");
            assert_eq!(args, ["symbolic-ref", "--short", "HEAD"]);
            Ok(CommandOutput { success: true, stdout: "main\n".to_string(), stderr: String::new() })
        }
    }
    let dir = TempDir::new().unwrap();
    assert_eq!(get_current_branch::<GitSaysMain>(dir.path()).as_deref(), Some("main"));
    assert_eq!(get_current_branch::<GitFails>(dir.path()), None);
}

/// The repository is untrusted input, so `HEAD` is read only when it is a
/// plain file of a plausible size. Anything else falls back to `git
/// symbolic-ref` rather than being read.
#[test]
fn oversized_git_metadata_is_not_read() {
    let repo = repo_with_head(&"ref: refs/heads/main\n".repeat(1024));
    assert_eq!(get_current_branch::<GitFails>(repo.path()), None);

    let dir = TempDir::new().unwrap();
    let git_dir = dir.path().join("real-git-dir");
    fs::create_dir(&git_dir).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/linked\n").unwrap();
    let worktree = dir.path().join("worktree");
    fs::create_dir(&worktree).unwrap();
    fs::write(worktree.join(".git"), format!("gitdir: {}\n", "x".repeat(9000))).unwrap();
    assert_eq!(get_current_branch::<GitFails>(&worktree), None);
}

/// A `HEAD` that is not a plain file — a symlink here, a FIFO or a device
/// in the cases this stands in for — is never opened, so a repository
/// cannot make the read block or balloon.
#[cfg(unix)]
#[test]
fn a_head_that_is_not_a_plain_file_is_not_read() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("elsewhere");
    fs::write(&target, "ref: refs/heads/sneaky\n").unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::create_dir(repo.join(".git")).unwrap();
    std::os::unix::fs::symlink(&target, repo.join(".git/HEAD")).unwrap();

    assert_eq!(get_current_branch::<GitFails>(&repo), None);
    assert!(!is_head_detached::<NoGit>(&repo), "refused metadata must not be queried by Git");
    assert!(
        super::get_branch_candidates_from_git::<NoGit>(&repo).is_empty(),
        "refused metadata must not be queried by Git for branch candidates",
    );
}

/// A FIFO at `HEAD` must be refused rather than opened: a plain `open`
/// on one blocks until a writer appears, which would hang every install
/// that consults the branch.
///
/// Refused metadata is a dead end rather than a reason to fall back, so
/// the real provider is used here: handing the same path to `git
/// symbolic-ref` would only move the same blocking read into the
/// subprocess. Both tests hang rather than fail if either guard is
/// dropped, which is the only way to observe an open that never returns.
#[cfg(unix)]
#[test]
fn a_head_that_is_a_fifo_does_not_block_the_read() {
    let repo = repo_with_fifo_head();

    assert_eq!(get_current_branch::<GitFails>(repo.path()), None);
    assert_eq!(get_current_branch::<Host>(repo.path()), None);
    assert!(super::get_branch_candidates_from_git::<NoGit>(repo.path()).is_empty());
    assert!(super::get_branch_candidates_from_git::<Host>(repo.path()).is_empty());
}

/// The `.git` pointer file of a worktree gets the same treatment: a FIFO
/// there is refused, and git is not asked to read it either.
#[cfg(unix)]
#[test]
fn a_gitdir_pointer_that_is_a_fifo_does_not_block_the_read() {
    let dir = TempDir::new().unwrap();
    let worktree = dir.path().join("worktree");
    fs::create_dir(&worktree).unwrap();
    make_fifo(&worktree.join(".git"));

    assert_eq!(get_current_branch::<Host>(&worktree), None);
    assert!(super::get_branch_candidates_from_git::<NoGit>(&worktree).is_empty());
    assert!(super::get_branch_candidates_from_git::<Host>(&worktree).is_empty());
}

#[cfg(unix)]
fn repo_with_fifo_head() -> TempDir {
    let repo = TempDir::new().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    make_fifo(&repo.path().join(".git/HEAD"));
    repo
}

#[cfg(unix)]
fn make_fifo(path: &std::path::Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed");
}

#[test]
fn a_failed_head_verification_is_not_detached() {
    let repo = repo_with_head("0123456789abcdef0123456789abcdef01234567\n");
    assert!(
        !is_head_detached::<GitFails>(repo.path()),
        "a failed Git query must not confirm detachment",
    );
}

#[test]
fn ci_env_detection_respects_variables() {
    struct MockPnpmGitBranch;
    impl super::EnvVar for MockPnpmGitBranch {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_GIT_BRANCH" => Some("refs/heads/custom-override".to_string()),
                _ => None,
            }
        }
    }
    assert_eq!(
        super::get_branch_from_ci_env::<MockPnpmGitBranch>(Path::new("/workspace/repo")).as_deref(),
        Some("custom-override"),
    );

    struct MockGithubCi;
    impl super::EnvVar for MockGithubCi {
        fn var(name: &str) -> Option<String> {
            match name {
                "CI" => Some("true".to_string()),
                "GITHUB_WORKSPACE" => Some("/workspace/repo".to_string()),
                "GITHUB_HEAD_REF" => Some("pr-branch".to_string()),
                _ => None,
            }
        }
    }
    assert_eq!(
        super::get_branch_from_ci_env::<MockGithubCi>(Path::new("/workspace/repo")).as_deref(),
        Some("pr-branch"),
    );
    assert_eq!(super::get_branch_from_ci_env::<MockGithubCi>(Path::new("/other/location")), None);

    struct MockGithubTag;
    impl super::EnvVar for MockGithubTag {
        fn var(name: &str) -> Option<String> {
            match name {
                "CI" => Some("true".to_string()),
                "GITHUB_WORKSPACE" => Some("/workspace/repo".to_string()),
                "GITHUB_REF_TYPE" => Some("tag".to_string()),
                "GITHUB_REF_NAME" => Some("v1.0.0".to_string()),
                _ => None,
            }
        }
    }
    assert_eq!(super::get_branch_from_ci_env::<MockGithubTag>(Path::new("/workspace/repo")), None);

    struct MockEmptyVars;
    impl super::EnvVar for MockEmptyVars {
        fn var(name: &str) -> Option<String> {
            match name {
                "CI" => Some("true".to_string()),
                "GITHUB_WORKSPACE" => Some("/workspace/repo".to_string()),
                "GITHUB_HEAD_REF" => Some("   ".to_string()),
                "GITHUB_REF_NAME" => Some(String::new()),
                "CI_COMMIT_BRANCH" => Some("commit-branch".to_string()),
                _ => None,
            }
        }
    }
    assert_eq!(
        super::get_branch_from_ci_env::<MockEmptyVars>(Path::new("/workspace/repo")).as_deref(),
        Some("commit-branch"),
    );
}

#[test]
fn get_branch_candidates_parses_git_output() {
    struct GitBranchOutput;
    impl RunCommand for GitBranchOutput {
        fn run(_: &str, args: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            if args.contains(&"name-rev") {
                Ok(CommandOutput {
                    success: true,
                    stdout: "feat/my-branch~2\n".to_string(),
                    stderr: String::new(),
                })
            } else if args.contains(&"branch") {
                Ok(CommandOutput {
                    success: true,
                    stdout: "(HEAD detached at 1234567)\nfeat/my-branch\nremotes/origin/main\n"
                        .to_string(),
                    stderr: String::new(),
                })
            } else {
                Ok(CommandOutput { success: false, stdout: String::new(), stderr: String::new() })
            }
        }
    }
    let candidates = super::get_branch_candidates_from_git::<GitBranchOutput>(Path::new("."));
    assert_eq!(candidates, vec!["feat/my-branch", "main"]);
}
