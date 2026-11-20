use pnpr_package_name::Ecosystem;
use std::fmt::Display;

/// The origin of the mock listening on `port`, with a trailing `/`.
pub fn port_to_url(port: impl Display) -> String {
    format!("http://localhost:{port}/")
}

/// The npm registry URL, with a trailing `/`, of a pnpr server at `origin`.
#[must_use]
pub fn npm_registry_url(origin: &str) -> String {
    format!("{}{}/", origin.trim_end_matches('/'), Ecosystem::Npm.base_path())
}
