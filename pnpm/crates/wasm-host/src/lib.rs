//! Asynchronous host operations for the threaded WASI build.
//!
//! The supervisor retains request results until read or cancelled. Guest memory
//! is borrowed only during an import call, never while an operation is pending.

#[cfg(target_family = "wasm")]
pub use request::{HostError, Request, close_resource, request};

#[cfg(target_family = "wasm")]
mod bindings;
#[cfg(target_family = "wasm")]
mod request;

#[cfg(target_family = "wasm")]
#[must_use]
/// Returns the supervising Node process identifier shared by all guest threads.
pub fn process_id() -> u32 {
    static PROCESS_ID: std::sync::LazyLock<u32> = std::sync::LazyLock::new(|| {
        std::env::var("PNPM_WASM_PID")
            .expect("WASI host must supply PNPM_WASM_PID")
            .parse()
            .expect("PNPM_WASM_PID must be a process identifier")
    });
    *PROCESS_ID
}

#[cfg(target_family = "wasm")]
mod blocking;
#[cfg(target_family = "wasm")]
pub use blocking::block_on;

/// Runs one host operation synchronously on the calling guest worker.
#[cfg(target_family = "wasm")]
pub fn request_blocking(message: &serde_json::Value) -> Result<serde_json::Value, HostError> {
    block_on(request(message)?)
}

#[cfg(target_family = "wasm")]
mod terminal;
#[cfg(target_family = "wasm")]
pub use terminal::{confirm, input, password, stdin_is_terminal};
