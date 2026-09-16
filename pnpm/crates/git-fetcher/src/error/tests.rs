use super::GitFetcherError;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

#[test]
fn direct_checkout_errors_redact_credentials_without_changing_error_payloads() {
    let repo = "https://secret-user:secret-password@example.test/repo";
    let stderr = format!("fatal: cannot access '{repo}': denied\n\u{1b}[31m");
    let errors = [
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
        if let GitFetcherError::GitExec { stderr: original, .. } = error {
            assert_eq!(original, stderr);
        }
    }
}
