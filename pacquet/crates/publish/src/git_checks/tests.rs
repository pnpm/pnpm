use super::{GitCheckError, run_git_checks};
use crate::capabilities::{CommandOutput, ConfirmPrompt, RunCommand};
use std::{io, path::Path};
use tempfile::TempDir;

/// Create a scratch directory holding a `.git/HEAD` with the given contents.
fn repo_with_head(head: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".git/HEAD"), head).unwrap();
    dir
}

fn ok(stdout: &str) -> io::Result<CommandOutput> {
    Ok(CommandOutput { success: true, stdout: stdout.to_owned(), stderr: String::new() })
}

fn fail() -> io::Result<CommandOutput> {
    Ok(CommandOutput { success: false, stdout: String::new(), stderr: String::new() })
}

#[test]
fn skips_when_disabled() {
    struct Sys;
    impl RunCommand for Sys {
        fn run(_: &str, _: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            unreachable!("git is not invoked when checks are disabled")
        }
    }
    impl ConfirmPrompt for Sys {
        fn confirm(_: &str) -> bool {
            unreachable!()
        }
    }
    assert!(run_git_checks::<Sys>(Path::new("/"), false, None).is_ok());
}

#[test]
fn skips_when_not_a_git_repo() {
    struct Sys;
    impl RunCommand for Sys {
        fn run(_: &str, args: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            assert_eq!(args, ["rev-parse", "--git-dir"]);
            fail()
        }
    }
    impl ConfirmPrompt for Sys {
        fn confirm(_: &str) -> bool {
            unreachable!()
        }
    }
    assert!(run_git_checks::<Sys>(Path::new("/"), true, None).is_ok());
}

#[test]
fn errors_on_unclean_tree() {
    struct Sys;
    impl RunCommand for Sys {
        fn run(_: &str, args: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            match args[0] {
                "rev-parse" => ok(""),
                "status" => ok(" M file.txt\n"),
                other => unreachable!("unexpected git call: {other}"),
            }
        }
    }
    impl ConfirmPrompt for Sys {
        fn confirm(_: &str) -> bool {
            unreachable!()
        }
    }
    let err = run_git_checks::<Sys>(Path::new("/"), true, None).unwrap_err();
    assert!(matches!(err, GitCheckError::Unclean));
}

#[test]
fn errors_on_detached_head() {
    let repo = repo_with_head("0123456789abcdef0123456789abcdef01234567\n");
    struct Sys;
    impl RunCommand for Sys {
        fn run(_: &str, args: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            match args[0] {
                "rev-parse" => ok(""),
                "status" => ok(""),
                other => unreachable!("unexpected git call: {other}"),
            }
        }
    }
    impl ConfirmPrompt for Sys {
        fn confirm(_: &str) -> bool {
            unreachable!()
        }
    }
    let err = run_git_checks::<Sys>(repo.path(), true, None).unwrap_err();
    assert!(matches!(err, GitCheckError::UnknownBranch { .. }));
}

#[test]
fn errors_on_wrong_branch_when_declined() {
    let repo = repo_with_head("ref: refs/heads/feature\n");
    struct Sys;
    impl RunCommand for Sys {
        fn run(_: &str, args: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            match args[0] {
                "rev-parse" => ok(""),
                "status" => ok(""),
                other => unreachable!("remote check is not reached: {other}"),
            }
        }
    }
    impl ConfirmPrompt for Sys {
        fn confirm(_: &str) -> bool {
            false
        }
    }
    let err = run_git_checks::<Sys>(repo.path(), true, None).unwrap_err();
    assert!(matches!(err, GitCheckError::NotCorrectBranch { .. }));
}

#[test]
fn passes_on_publish_branch_with_clean_remote() {
    let repo = repo_with_head("ref: refs/heads/main\n");
    struct Sys;
    impl RunCommand for Sys {
        fn run(_: &str, args: &[&str], _: Option<&Path>) -> io::Result<CommandOutput> {
            match args[0] {
                "rev-parse" => ok(""),
                "status" => ok(""),
                "rev-list" => ok("0\n"),
                other => unreachable!("unexpected git call: {other}"),
            }
        }
    }
    impl ConfirmPrompt for Sys {
        fn confirm(_: &str) -> bool {
            unreachable!("no prompt when already on a publish branch")
        }
    }
    assert!(run_git_checks::<Sys>(repo.path(), true, None).is_ok());
}
