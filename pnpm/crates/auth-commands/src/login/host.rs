use std::{future::Future, io, path::Path};

use pacquet_network_web_auth::{
    Clock, EnterKeyListener, Host as WebAuthHost, OpenUrl, PromptError, PromptOtp, Sleep,
    StdinIsTty, StdoutIsTty, WebAuthFetch, WebAuthFetchError, WebAuthFetchOptions,
    WebAuthFetchResponse,
};

use super::prompt::{PromptInput, PromptPassword};
use crate::logout::{FsReadToString, FsWrite};

/// Production provider for `pnpm login`. The credential prompts and `auth.ini`
/// I/O are real; every OTP / web-authentication capability delegates to
/// [`pacquet_network_web_auth::Host`], the shared production provider for that
/// flow.
pub struct Host;

impl FsReadToString for Host {
    fn read_to_string(path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

impl FsWrite for Host {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        pacquet_fs::write_atomic(path, bytes)
    }
}

impl PromptInput for Host {
    fn prompt_input(message: &str) -> Result<String, dialoguer::Error> {
        dialoguer::Input::<String>::new().with_prompt(message).allow_empty(true).interact_text()
    }
}

impl PromptPassword for Host {
    fn prompt_password(message: &str) -> Result<String, dialoguer::Error> {
        // `allow_empty_password(true)` mirrors `@inquirer/prompts` `password`,
        // which returns an empty string on a bare Enter. Without it dialoguer
        // loops until non-empty, so pnpm's empty-password path
        // (`LOGIN_MISSING_CREDENTIALS`) would be unreachable.
        dialoguer::Password::new().with_prompt(message).allow_empty_password(true).interact()
    }
}

impl Clock for Host {
    fn now_ms() -> u64 {
        WebAuthHost::now_ms()
    }
}

impl Sleep for Host {
    fn sleep_ms(ms: u64) -> impl Future<Output = ()> {
        WebAuthHost::sleep_ms(ms)
    }
}

impl WebAuthFetch for Host {
    fn fetch(
        url: &str,
        options: &WebAuthFetchOptions,
    ) -> impl Future<Output = Result<WebAuthFetchResponse, WebAuthFetchError>> {
        WebAuthHost::fetch(url, options)
    }
}

impl StdinIsTty for Host {
    fn stdin_is_tty() -> bool {
        WebAuthHost::stdin_is_tty()
    }
}

impl StdoutIsTty for Host {
    fn stdout_is_tty() -> bool {
        WebAuthHost::stdout_is_tty()
    }
}

impl OpenUrl for Host {
    fn open_url(url: &str) -> io::Result<()> {
        WebAuthHost::open_url(url)
    }
}

impl EnterKeyListener for Host {
    type Handle = <WebAuthHost as EnterKeyListener>::Handle;
    fn listen() -> io::Result<Self::Handle> {
        WebAuthHost::listen()
    }
}

impl PromptOtp for Host {
    fn input(message: &str) -> impl Future<Output = Result<Option<String>, PromptError>> {
        WebAuthHost::input(message)
    }
}
