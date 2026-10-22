#[cfg(not(target_family = "wasm"))]
pub use which::*;

#[cfg(target_family = "wasm")]
mod host;
#[cfg(target_family = "wasm")]
pub use host::{which, which_all, which_in, which_in_global};
#[cfg(target_family = "wasm")]
pub use which::{Error, Result};
