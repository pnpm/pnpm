---
"@pnpm/napi": patch
---

Fixed several mismatches between the `@pnpm/napi` API and its type declarations:

- `install` with a single project no longer reads the `package.json` files of the other workspace projects from disk.
- `install` with `ignorePackageManifest` accepts an empty `projects` list.
- `install` rejects an unknown `nodeLinker`, `packageImportMethod`, or `networkConfig.localAddress` value with `ERR_PNPM_INVALID_OPTION`.
- `resolveDependency` applies `proxyConfig` and `networkConfig`, returns the package's `resolution`, and keeps the `code` of pnpm errors.
