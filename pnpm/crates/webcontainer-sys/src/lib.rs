//! Raw declarations of the imports the `WebContainer` supervisor provides to
//! the WASI guest.
//!
//! A `-sys` crate as the [`unsafe` policy](../../../UNSAFE_POLICY.md)
//! defines one: it declares the foreign ABI and nothing else. Every item
//! here is `unsafe` to call, states no precondition of its own, and
//! translates no error. The safe API over it is
//! [`pnpm-ffi-webcontainer`](../ffi-webcontainer).
//!
//! One module per import module, named after it.

#[cfg(target_family = "wasm")]
pub mod pnpm_fs;
#[cfg(target_family = "wasm")]
pub mod pnpm_host;
