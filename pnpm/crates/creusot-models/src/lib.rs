//! Creusot contracts for the functions that pnpm's verified crates call and
//! `creusot-std` leaves unspecified: some of the standard library's, and
//! those of third-party crates.
//!
//! Creusot trusts a contract here: it assumes the postcondition and proves
//! nothing about the body. Two things stand in for that proof:
//!
//! - `tests.rs` runs each real function on generated inputs and checks the
//!   postcondition, so a contract that claims more than the function does
//!   fails a test, and one that no result satisfies cannot pass.
//! - `clients.rs` holds verified callers, so a contract too weak to be
//!   used fails to prove.
//!
//! A logic axiom such as `str_model::str_size_bound` describes no function,
//! so neither check applies to it, and its documentation gives its source.
//!
//! Everything but the tests is compiled only by `cargo creusot`. A verified
//! crate depends on this one under `[target.'cfg(creusot)'.dependencies]`
//! and loads it with `use pnpm_creusot_models as _;`. See
//! `pnpm/VERIFICATION.md`.

#![cfg_attr(creusot, feature(pattern))]

#[cfg(creusot)]
pub mod clients;
#[cfg(creusot)]
pub mod str_model;

#[cfg(creusot)]
mod first_char_model;
#[cfg(creusot)]
mod pipe_model;
#[cfg(creusot)]
mod slice_model;

#[cfg(test)]
mod tests;
