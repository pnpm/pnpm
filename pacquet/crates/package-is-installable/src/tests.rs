//! Unit tests for package-installability checks: platform support,
//! engine (Node/npm) version constraints, and inferring the platform
//! from a package name. Each area has its own submodule below.

mod check_engine;
mod check_platform;
mod infer_platform_from_package_name;
mod package_is_installable;
