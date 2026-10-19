---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add` now reads the package name correctly from a tarball URL under a registry URL that has a path, such as `https://example.com/npm/`. Before, the registry path became part of the name, so a scoped package failed with `ERR_PNPM_INVALID_PACKAGE_NAME`.
