use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use std::env::consts::{
    ARCH,
    OS,
};

#[derive(Debug, Display, Error, Diagnostic, Clone, Copy, PartialEq, Eq)]
#[display("pnpz has no packages for {ARCH}-{OS}")]
#[diagnostic(code(ERR_PNPZ_UNSUPPORTED_HOST))]
pub struct UnsupportedHostError;

/// The name nixpkgs gives the platform pnpz runs on, such as `x86_64-linux`.
pub fn host_system() -> Result<String, UnsupportedHostError> {
    let os = match OS {
        "linux" => "linux",
        "macos" => "darwin",
        _ => return Err(UnsupportedHostError),
    };
    match ARCH {
        "x86_64" | "aarch64" => Ok(format!("{ARCH}-{os}")),
        _ => Err(UnsupportedHostError),
    }
}
