use super::{CommandOutput, Host, RunCommand, get_current_branch};
use std::{fs, io, path::Path};
use tempfile::TempDir;

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
    let status = std::process::Command::new("mkfifo").arg(path).status().expect("run mkfifo");
    assert!(status.success(), "mkfifo failed");
}
