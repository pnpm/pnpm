use pnpm_wasm_host::HostError;
pub(crate) use pnpm_wasm_host::block_on;
use serde_json::Value;
use std::io;

pub(crate) fn request(message: &Value) -> io::Result<Value> {
    block_on(pnpm_wasm_host::request(message).map_err(error)?).map_err(error)
}

pub(crate) async fn request_async(message: &Value) -> io::Result<Value> {
    pnpm_wasm_host::request(message).map_err(error)?.await.map_err(error)
}

pub(crate) fn error(error: HostError) -> io::Error {
    let kind = match error.code.as_deref() {
        Some("ENOENT" | "ESRCH") => io::ErrorKind::NotFound,
        Some("EACCES" | "EPERM") => io::ErrorKind::PermissionDenied,
        Some("EPIPE") => io::ErrorKind::BrokenPipe,
        Some("EINVAL") => io::ErrorKind::InvalidInput,
        Some("ESHELLPARSE") => io::ErrorKind::InvalidData,
        Some("ABORT_ERR") => io::ErrorKind::Interrupted,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error)
}
