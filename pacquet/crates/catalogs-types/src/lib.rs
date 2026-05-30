//! Pacquet port of pnpm's
//! [`@pnpm/catalogs.types`](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/types/src/index.ts).
//!
//! Defines [`Catalog`] and [`Catalogs`], the normalized in-memory shape
//! every other catalogs crate consumes. Both are plain typed maps in
//! upstream — `interface Catalog { [name: string]: string | undefined }`
//! and `interface Catalogs { default?: Catalog; [name: string]: Catalog
//! | undefined }` — so pacquet exposes them as `BTreeMap` aliases. The
//! `"default"` catalog is just the well-known key inside [`Catalogs`];
//! no separate field is needed.

use std::collections::BTreeMap;

/// The well-known name of the default catalog. Matches the literal pnpm
/// uses everywhere a catalog name is implicit.
pub const DEFAULT_CATALOG_NAME: &str = "default";

/// One catalog: a map of dependency name to version specifier.
///
/// Mirrors upstream's `Catalog` interface
/// ([source](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/types/src/index.ts#L27-L29)).
pub type Catalog = BTreeMap<String, String>;

/// The full set of catalogs parsed from `pnpm-workspace.yaml`. The
/// default catalog lives at the [`DEFAULT_CATALOG_NAME`] key; any other
/// key is a user-defined named catalog.
///
/// Mirrors upstream's `Catalogs` interface
/// ([source](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/types/src/index.ts#L1-L25)).
pub type Catalogs = BTreeMap<String, Catalog>;
