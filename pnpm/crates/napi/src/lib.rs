//! Node.js NAPI bindings for the pnpm v12 Rust engine (pacquet).
//!
//! This cdylib exposes pnpm's programmatic engine surface — pack, dependency
//! resolution, install, rebuild, and peer-dependency checks — to a Node.js
//! host through napi-rs. The reference consumer is Bit, which drives pnpm
//! entirely through its programmatic API. The JS-facing contract lives in the
//! wrapper package at `pnpm/npm/napi/index.d.ts`; the design rationale
//! is in `pnpm/plans/NAPI.md`.
//!
//! Only the *engine* is bound here. Pure data utilities that operate on
//! in-memory objects or the (byte-stable) on-disk lockfile/store formats stay
//! as JS packages in the consumer.

// napi-derive expands `#[napi]` into an FFI trampoline containing a
// `NapiRefContainer` whose last field is a `[napi_ref; 0]`. clippy's
// nursery `trailing_empty_array` lint fires on that generated struct; it is
// not our code and cannot be annotated at the definition site.
#![allow(
    clippy::trailing_empty_array,
    reason = "napi-derive generates a trailing zero-sized array in its FFI trampoline, which cannot be annotated at the definition site"
)]

mod config;
mod dependents;
mod error;
mod hooks;
mod install;
mod lockfile;
mod native_reporter;
mod pack;
mod read_config;
mod reporter_bridge;
mod resolve;
mod specifier;

pub use dependents::{DependentsOptions, RenderDependentsInput, get_dependents, render_dependents};
pub use install::{
    InstallOptions, InstallResult, InstallStatsResult, NodeApiProject, get_peer_dependency_issues,
    install, rebuild,
};
pub use lockfile::{
    FilterLockfileOptions, ReadLockfileOptions, WriteLockfileOptions, filter_lockfile_by_importers,
    read_lockfile, read_modules_manifest, write_lockfile,
};
use napi_derive::napi;
pub use native_reporter::ReporterOptions;
pub use pack::{PackOptions, PackResult, pack};
pub use read_config::{ReadConfigOptions, ResolvedConfig, ResolvedRegistry, read_config};
pub use resolve::{
    ResolveDependencyOptions, ResolveDependencyResult, WantedDependencyInput, resolve_dependency,
};
pub use specifier::{ParsedBareSpecifier, parse_bare_specifier};

/// Version of the underlying Rust engine (pacquet). Exposed as a function
/// rather than a const so napi maps it to a stable `engineVersion()` export.
#[napi(js_name = "engineVersion")]
#[must_use]
pub fn engine_version() -> &'static str {
    pnpm_config::PNPM_VERSION
}

/// Honor the same `TRACE` env var the pacquet CLI honors: an addon
/// embedded in a Node host has no `main` of its own, so the subscriber
/// is installed when the module loads.
#[napi_derive::module_init]
fn init_tracing() {
    pnpm_diagnostics::enable_tracing_by_env();
}

/// No-op stubs for the napi runtime symbols the `#[napi]` trampolines
/// reference, defined only for the crate's unit-test binary so it is
/// self-contained. See the module for the full rationale.
#[cfg(test)]
mod napi_runtime_test_stubs;
