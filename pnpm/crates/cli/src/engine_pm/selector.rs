//! Rewrite a `<tool>[@<spec>]` request into the selector that installs
//! the tool itself.
//!
//! `yarn` on npm stops at Classic, Yarn 6 is not published there at all,
//! and `node` / `deno` are wrappers that download a build. Asking for one
//! of those by name therefore has to become a different selector before
//! the install pipeline sees it — an npm alias for the lines that are
//! published under another name, the `runtime:` protocol for the ones
//! that ship as platform archives.

use crate::engine_pm::channel::{BinaryChannel, Channel as ToolChannel, PackageManager};
use pnpm_package_manifest::{
    is_runtime_alias,
    package_manager_spec::{is_version_request, split_spec},
};
use pnpm_rust_toolchain::Channel;

/// The install selector for `request`, or `None` when it names no tool
/// pnpm manages and the request stands as written.
///
/// pnpm itself is deliberately absent: installing it is `self-update`'s
/// job, and the global install path refuses it before reaching here.
#[must_use]
pub(crate) fn tool_install_selector(request: &str) -> Option<String> {
    let (name, version_spec) = split_request(request);
    // A specifier that locates a package — `node@runtime:22`, an explicit
    // `yarn@npm:@yarnpkg/cli-dist@4`, `yarn@yarnpkg/berry` — already says
    // what to install, and rewriting it would nest one locator inside
    // another.
    if !is_version_request(version_spec) {
        return None;
    }
    if let Some(pm) = PackageManager::parse(name).filter(|pm| *pm != PackageManager::Pnpm) {
        return package_manager_selector(pm, name, version_spec);
    }
    // `bun` is both, and its package-manager channel already answered.
    is_runtime_alias(name).then(|| runtime_selector(name, version_spec))
}

fn package_manager_selector(pm: PackageManager, name: &str, version_spec: &str) -> Option<String> {
    match pm.channel(version_spec) {
        // Published under its own name: the request already selects it.
        ToolChannel::Registry { package } if package == name => None,
        // Published under another name — install it under the name the
        // user asked for, so its bins land where they expect.
        ToolChannel::Registry { package } => Some(format!("{name}@npm:{package}@{version_spec}")),
        ToolChannel::Binary(BinaryChannel::Bun | BinaryChannel::Yarn) => {
            Some(runtime_selector(name, version_spec))
        }
    }
}

/// The `runtime:` protocol selector, which resolves platform archives and
/// records the pin as a `runtime` engine rather than a dependency.
fn runtime_selector(name: &str, version_spec: &str) -> String {
    format!("{name}@runtime:{version_spec}")
}

/// The Rust toolchain `rust[@<channel>]` asks for, or `None` when the
/// request names something else, such as `rust@npm:rust` naming the npm
/// package.
///
/// The npm package named `rust` is a wrapper that downloads a toolchain,
/// as the `node` package is, so the name stands for the toolchain itself.
/// A bare `rust` and `rust@latest` ask for `stable`.
pub(crate) fn rust_toolchain_request(request: &str) -> Option<Result<Channel, RustChannelError>> {
    let (name, version_spec) = split_spec(request);
    if name != "rust" || version_spec.is_some_and(|spec| !is_version_request(spec)) {
        return None;
    }
    let spec = match version_spec {
        None | Some("" | "latest") => "stable",
        Some(spec) => spec,
    };
    Some(Channel::parse(spec).ok_or_else(|| RustChannelError { spec: spec.to_string() }))
}

/// A `rust@<spec>` naming no release channel.
#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[display(r#""{spec}" is not a Rust release channel"#)]
#[diagnostic(
    code(ERR_PNPM_RUST_TOOLCHAIN_CHANNEL),
    help(
        "Name a version (1.95.0 or 1.95), stable, beta, nightly, or a dated channel such as nightly-2026-01-01."
    )
)]
pub(crate) struct RustChannelError {
    #[error(not(source))]
    spec: String,
}

/// Split `<name>[@<spec>]`, defaulting to the tool's current line. A
/// scoped name is never a tool, so it keeps its own `@` and asks for
/// nothing.
fn split_request(request: &str) -> (&str, &str) {
    let (name, version_spec) = split_spec(request);
    if name.starts_with('@') {
        return (request, "");
    }
    (name, version_spec.unwrap_or("latest"))
}

#[cfg(test)]
mod tests;
