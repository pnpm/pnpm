use super::{EnterKeyListener, Host, OpenUrl, OpenUrlAndWait, PromptError, PromptOtp};
use std::{future, io};

impl OpenUrl for Host {
    fn open_url(_url: &str) -> io::Result<()> {
        Err(browser_unavailable())
    }
}

impl OpenUrlAndWait for Host {
    fn open_url_and_wait(_url: &str) -> io::Result<()> {
        Err(browser_unavailable())
    }
}

impl EnterKeyListener for Host {
    type Handle = future::Pending<()>;

    fn listen() -> io::Result<Self::Handle> {
        Err(browser_unavailable())
    }
}

impl PromptOtp for Host {
    async fn input(message: &str) -> Result<Option<String>, PromptError> {
        let request = pnpm_wasm_host::request(&serde_json::json!({
            "operation": "terminal.prompt",
            "message": message,
        }))
        .map_err(|error| PromptError::Other { reason: error.to_string() })?;
        let response =
            request.await.map_err(|error| PromptError::Other { reason: error.to_string() })?;
        if response.get("cancelled").and_then(serde_json::Value::as_bool) == Some(true) {
            return Err(PromptError::Cancelled);
        }
        match response.get("value") {
            Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
            _ => Err(PromptError::Other { reason: "Invalid terminal prompt response".into() }),
        }
    }
}

fn browser_unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "WebContainers cannot open a browser window. Open the printed authentication URL in your browser.",
    )
}
