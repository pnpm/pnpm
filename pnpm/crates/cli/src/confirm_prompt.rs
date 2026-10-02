#[cfg(not(target_family = "wasm"))]
pub(crate) fn confirm(message: &str, default: Option<bool>) -> dialoguer::Result<bool> {
    let prompt = dialoguer::Confirm::new().with_prompt(message);
    let prompt = match default {
        Some(default) => prompt.default(default),
        None => prompt,
    };
    prompt.interact()
}

#[cfg(target_family = "wasm")]
pub(crate) fn confirm(message: &str, default: Option<bool>) -> dialoguer::Result<bool> {
    pnpm_wasm_host::confirm(message, default).map_err(dialoguer::Error::IO)
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn stdin_is_terminal() -> std::io::Result<bool> {
    use std::io::IsTerminal;
    Ok(std::io::stdin().is_terminal())
}

#[cfg(target_family = "wasm")]
pub(crate) use pnpm_wasm_host::stdin_is_terminal;
