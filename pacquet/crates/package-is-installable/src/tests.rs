//! Ports of upstream's unit tests:
//! - `config/package-is-installable/test/checkPlatform.ts`
//! - `config/package-is-installable/test/checkEngine.ts`
//! - `config/package-is-installable/test/inferPlatformFromPackageName.ts`
//!
//! All live under
//! <https://github.com/pnpm/pnpm/tree/34875b2d7c/config/package-is-installable/test>.

mod check_engine;
mod check_platform;
mod infer_platform_from_package_name;
mod package_is_installable;
