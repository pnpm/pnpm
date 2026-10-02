//! Commands for authenticating with npm registries: `login` / `adduser` and
//! `logout`.

#[cfg(target_family = "wasm")]
extern crate pnpm_http as reqwest;

pub mod config_yaml;
pub mod login;
pub mod logout;

mod ini;
mod registry_url;
