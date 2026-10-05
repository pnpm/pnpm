---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_PACKAGE_MANIFEST_INVALID_ATTRIBUTE` when a project declares a dependency whose specifier is not a string, such as `"is-positive": 42`. Before, the dependency was silently left out of the lockfile. A `readPackage` hook can still correct the specifier.
