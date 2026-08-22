use super::{CommandOutput, RunCommand, get_current_branch};
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
    struct GitFails;
    impl RunCommand for GitFails {
        fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            Ok(CommandOutput { success: false, stdout: String::new(), stderr: String::new() })
        }
    }

    let dir = TempDir::new().unwrap();
    assert_eq!(get_current_branch::<GitSaysMain>(dir.path()).as_deref(), Some("main"));
    assert_eq!(get_current_branch::<GitFails>(dir.path()), None);
}
