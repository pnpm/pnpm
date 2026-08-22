---
"pacquet": minor
---

The Rust engine now checks that a package read back from the store is the package it was recorded as. When the tarball's `package.json` names a different name or version than the store entry was keyed for — a broken lockfile, or a registry serving content that doesn't match its metadata — the install fails with `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE`. Set the new `strictStorePkgContentCheck` setting to `false` to downgrade the failure to a warning and install from the entry anyway [#12042](https://github.com/pnpm/pnpm/issues/12042).
