//! `login` test: the non-interactive-terminal guard.

use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use pacquet_network_web_auth_testing::web_auth_fake;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;

use super::{
    LoginError, login,
    test_support::{PromptScript, ReadScript, client, login_fake, opts},
};

#[tokio::test]
async fn should_throw_in_non_interactive_terminal() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_stdin_tty(false);

    let config_dir = Path::new("/mock/config");
    let err =
        login::<FakeHost, RecordingReporter>(&client(), opts("https://example.org", config_dir))
            .await
            .unwrap_err();

    assert!(matches!(err, LoginError::NonInteractive), "got {err:?}");
    assert_eq!(err.to_string(), "The login command requires an interactive terminal");
    assert_eq!(
        err.pipe_ref(miette::Diagnostic::code).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_NON_INTERACTIVE"),
    );
}
