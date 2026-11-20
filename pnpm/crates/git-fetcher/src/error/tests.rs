#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

use miette::Diagnostic;

use super::GitFetcherError;

#[test]
fn direct_checkout_errors_redact_credentials_without_changing_error_payloads() {
    let repo = "https://secret-user:secret-password@example.test/repo";
    let stderr = format!("fatal: cannot access '{repo}': denied\n\u{1b}[31m");
    let errors = [
        GitFetcherError::Fetch {
            package: "demo".to_string(),
            repo: repo.to_string(),
            stderr: stderr.clone(),
        },
        GitFetcherError::FetchOverSsh {
            package: "demo".to_string(),
            repo: repo.to_string(),
            host: "example.test".to_string(),
            stderr: stderr.clone(),
        },
        GitFetcherError::InvalidRepo { repo: repo.to_string() },
        GitFetcherError::InvalidCommit { commit: "invalid".to_string(), repo: repo.to_string() },
        GitFetcherError::GitExec {
            operation: "clone",
            stderr: stderr.clone(),
            status: std::process::ExitStatus::from_raw(1),
        },
    ];
    for error in errors {
        let diagnostic = error.to_string();
        assert!(!diagnostic.contains("secret-user"), "{diagnostic}");
        assert!(!diagnostic.contains("secret-password"), "{diagnostic}");
        assert!(!diagnostic.contains('\u{1b}'), "{diagnostic}");
        assert!(diagnostic.contains("example.test/repo"), "{diagnostic}");
        if let GitFetcherError::GitExec { stderr: original, .. }
        | GitFetcherError::Fetch { stderr: original, .. }
        | GitFetcherError::FetchOverSsh { stderr: original, .. } = error
        {
            assert_eq!(original, stderr);
        }
    }
}

#[test]
fn an_ssh_clone_failure_explains_a_publickey_refusal_and_how_to_re_record_https() {
    let error = GitFetcherError::FetchOverSsh {
        package: "@scope/pkg".to_string(),
        repo: "git@github.com:acme/widget.git".to_string(),
        host: "github.com".to_string(),
        stderr: "git@github.com: Permission denied (publickey).".to_string(),
    };

    let help = Diagnostic::help(&error).expect("SSH remediation").to_string();
    assert!(help.contains("needs an SSH key for github.com"), "{help}");
    assert!(help.contains("Permission denied (publickey)"), "{help}");
    assert!(help.contains("ssh-add -l"), "{help}");
    assert!(help.contains("pnpm update @scope/pkg"), "{help}");
    assert!(help.contains("do not re-resolve git dependencies"), "{help}");
}
