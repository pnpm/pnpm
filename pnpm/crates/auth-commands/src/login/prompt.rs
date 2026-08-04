use std::io;

use pacquet_network_web_auth::PromptError;

/// Read a visible credential line — the `dialoguer::Input` builder chain (the
/// username and email prompts). This is the blocking terminal read behind the
/// capability seam, and nothing more: the production impl is that bare chain.
///
/// The surrounding algorithm — running the blocking read off the async runtime,
/// selecting the visible or masked read by masking, and classifying a
/// [`dialoguer::Error`] into a [`PromptError`] — deliberately does *not* live
/// here or in [`PromptPassword`]. It lives in `prompt_line`, so each provider
/// stays a bare builder chain while a test fakes only the read and that real
/// algorithm still runs.
pub trait PromptInput {
    fn prompt_input(message: &str) -> Result<String, dialoguer::Error>;
}

/// Read a masked credential secret — the `dialoguer::Password` builder chain
/// (the password prompt). The masked counterpart of [`PromptInput`]; see there
/// for how the surrounding algorithm is kept out of the provider.
pub trait PromptPassword {
    fn prompt_password(message: &str) -> Result<String, dialoguer::Error>;
}

/// Whether [`prompt_line`] reads a visible line or a masked secret.
#[derive(Clone, Copy)]
pub(super) enum Masking {
    Visible,
    Masked,
}

/// Read one credential line off the async runtime (`dialoguer` is blocking):
/// run the injected [`PromptInput`] read for [`Masking::Visible`] or the
/// [`PromptPassword`] read for [`Masking::Masked`], then classify the outcome —
/// an interrupted prompt (Ctrl-C) maps to [`PromptError::Cancelled`] (mirroring
/// enquirer's `ExitPromptError`), a task panic or any other failure to
/// [`PromptError::Other`].
pub(super) async fn prompt_line<Sys: PromptInput + PromptPassword + 'static>(
    message: &str,
    masking: Masking,
) -> Result<String, PromptError> {
    let message = message.to_owned();
    tokio::task::spawn_blocking(move || match masking {
        Masking::Visible => Sys::prompt_input(&message),
        Masking::Masked => Sys::prompt_password(&message),
    })
    .await
    .map_err(|join_error| PromptError::Other { reason: join_error.to_string() })?
    .map_err(map_dialoguer_error)
}

fn map_dialoguer_error(error: dialoguer::Error) -> PromptError {
    match error {
        dialoguer::Error::IO(io) if io.kind() == io::ErrorKind::Interrupted => {
            PromptError::Cancelled
        }
        dialoguer::Error::IO(io) => PromptError::Other { reason: io.to_string() },
    }
}
