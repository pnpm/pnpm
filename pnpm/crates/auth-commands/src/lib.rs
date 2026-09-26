//! Commands for authenticating with npm registries: `login` / `adduser` and
//! `logout`.

pub mod config_yaml;
pub mod login;
pub mod logout;

mod ini;
mod registry_url;
