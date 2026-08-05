//! Defines [`Catalog`] and [`Catalogs`], the normalized in-memory shape
//! every other catalogs crate consumes. Both are plain typed maps —
//! name to version specifier, and catalog name to catalog — so pacquet
//! exposes them as `BTreeMap` aliases. The `"default"` catalog is just
//! the well-known key inside [`Catalogs`]; no separate field is needed.

use std::collections::BTreeMap;

/// The well-known name of the default catalog. Matches the literal pnpm
/// uses everywhere a catalog name is implicit.
pub const DEFAULT_CATALOG_NAME: &str = "default";

/// One catalog: a map of dependency name to version specifier.
pub type Catalog = BTreeMap<String, String>;

/// The full set of catalogs parsed from `pnpm-workspace.yaml`. The
/// default catalog lives at the [`DEFAULT_CATALOG_NAME`] key; any other
/// key is a user-defined named catalog.
pub type Catalogs = BTreeMap<String, Catalog>;
