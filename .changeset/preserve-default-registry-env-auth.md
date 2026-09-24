---
"@pnpm/config.reader": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

pnpm now keeps the configured default registry when `_auth` holds credentials for several registries and some of those registries serve package scopes.

Lockfile verification checks a tarball hosted on a scoped registry against that registry's metadata, unless the package's own scope has a registry assigned [pnpm/pnpm#15530](https://github.com/pnpm/pnpm/issues/15530).
