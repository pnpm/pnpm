//! Shared fixtures for the `login` tests.
//!
//! Holds the credential and `auth.ini` fake (`login_fake!`), its
//! scripted-response type aliases, and the helper constructors the three
//! scenario modules — non-interactive, web-login, and classic-login — build on.

use std::{
    io,
    path::{Path, PathBuf},
};

use pacquet_network::ThrottledClient;

use super::LoginOptions;
use crate::ini::IniSettings;

/// A scripted response for a credential prompt, keyed on the prompt message.
/// The error half is `dialoguer::Error` — exactly what the real terminal read
/// yields — so a script can drive [`super::prompt::prompt_line`]'s real error
/// classification (e.g. an interrupt mapping to a canceled login). `Send`
/// because `prompt_line` calls the fake read from a `spawn_blocking` thread.
pub(crate) type PromptScript = Box<dyn FnMut(&str) -> Result<String, dialoguer::Error> + Send>;

/// A scripted `auth.ini` read.
pub(crate) type ReadScript = Box<dyn FnMut(&Path) -> io::Result<String>>;

/// Expand the login-specific half of the `Sys` fake at the top of a test,
/// after [`web_auth_fake`]. `$fake` is the unit struct `web_auth_fake!`
/// generated (`FakeHost`); this adds the login capabilities to it over fn-local
/// state, always emitting `reset_login`, plus each `set_prompt_input` /
/// `set_prompt_password` / `set_ini_read` / `login_writes` helper named as an
/// extra argument. Naming only the helpers a scenario drives keeps every
/// emitted function used, so no `dead_code` suppression is needed.
///
/// The prompt scripts live in a fn-local `static` [`Mutex`], not `thread_local!`:
/// `prompt_line` runs `prompt_input` / `prompt_password` inside `spawn_blocking`,
/// so they execute on a blocking-pool thread where thread-local state would be
/// invisible. Each test's expansion has its own `static`, so tests stay
/// isolated. `auth.ini` I/O runs on the test thread and stays `thread_local!`.
macro_rules! login_fake {
    ($fake:ident $(, $helper:ident)* $(,)?) => {
        static PROMPT_INPUT: Mutex<Option<PromptScript>> = Mutex::new(None);
        static PROMPT_PASSWORD: Mutex<Option<PromptScript>> = Mutex::new(None);
        thread_local! {
            static INI_READ: RefCell<Option<ReadScript>> = const { RefCell::new(None) };
            static INI_WRITES: RefCell<Vec<(PathBuf, String)>> = const { RefCell::new(Vec::new()) };
        }

        impl crate::login::PromptInput for $fake {
            fn prompt_input(message: &str) -> Result<String, dialoguer::Error> {
                let mut script = PROMPT_INPUT.lock().expect("input script mutex");
                (script.as_mut().expect("an input script must be set"))(message)
            }
        }

        impl crate::login::PromptPassword for $fake {
            fn prompt_password(message: &str) -> Result<String, dialoguer::Error> {
                let mut script = PROMPT_PASSWORD.lock().expect("password script mutex");
                (script.as_mut().expect("a password script must be set"))(message)
            }
        }

        impl crate::logout::FsReadToString for $fake {
            fn read_to_string(path: &Path) -> io::Result<String> {
                INI_READ.with(|script| match script.borrow_mut().as_mut() {
                    Some(read) => read(path),
                    None => Ok(String::new()),
                })
            }
        }

        impl crate::logout::FsWrite for $fake {
            fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
                let text = String::from_utf8(bytes.to_vec()).expect("auth.ini is UTF-8");
                INI_WRITES.with(|writes| writes.borrow_mut().push((path.to_path_buf(), text)));
                Ok(())
            }
        }

        fn reset_login() {
            *PROMPT_INPUT.lock().expect("input script mutex") = None;
            *PROMPT_PASSWORD.lock().expect("password script mutex") = None;
            INI_READ.with(|cell| *cell.borrow_mut() = None);
            INI_WRITES.with(|writes| writes.borrow_mut().clear());
        }

        $( login_fake!(@helper $helper); )*
    };

    (@helper set_prompt_input) => {
        fn set_prompt_input(script: PromptScript) {
            *PROMPT_INPUT.lock().expect("input script mutex") = Some(script);
        }
    };
    (@helper set_prompt_password) => {
        fn set_prompt_password(script: PromptScript) {
            *PROMPT_PASSWORD.lock().expect("password script mutex") = Some(script);
        }
    };
    (@helper set_ini_read) => {
        fn set_ini_read(script: ReadScript) {
            INI_READ.with(|cell| *cell.borrow_mut() = Some(script));
        }
    };
    (@helper login_writes) => {
        fn login_writes() -> Vec<(PathBuf, String)> {
            INI_WRITES.with(|writes| writes.borrow().clone())
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `login_fake!` helper `",
            stringify!($unknown),
            "`; expected one of: set_prompt_input, set_prompt_password, set_ini_read, login_writes",
        ));
    };
}

pub(crate) use login_fake;

/// A throwaway HTTP client. Requests that reach it target the test's `mockito`
/// server (or, for the pre-network guards, are never sent at all).
pub(crate) fn client() -> ThrottledClient {
    ThrottledClient::default()
}

/// Build [`LoginOptions`] with retry / timeout knobs zeroed — the poll runs
/// against the fake clock, so the real values are irrelevant.
pub(crate) fn opts<'a>(registry: &'a str, config_dir: &'a Path) -> LoginOptions<'a> {
    LoginOptions {
        registry: Some(registry),
        scope: None,
        config_dir,
        fetch_retries: 0,
        fetch_retry_factor: 1,
        fetch_retry_mintimeout: 0,
        fetch_retry_maxtimeout: 0,
        fetch_timeout: 0,
    }
}

/// The `auth.ini` write [`login`] performed, parsed back into [`IniSettings`].
pub(crate) fn written_settings(writes: &[(PathBuf, String)]) -> IniSettings {
    let (_, text) = writes.first().expect("auth.ini was written");
    IniSettings::parse(text)
}

/// The classic-login prompt script the OTP tests share: username / email by
/// message, and a fixed password.
pub(crate) fn credential_prompts(username: &'static str, email: &'static str) -> PromptScript {
    Box::new(move |message| match message {
        "Username:" => Ok(username.to_owned()),
        "Email (this IS public):" => Ok(email.to_owned()),
        other => panic!("unexpected input prompt: {other}"),
    })
}
