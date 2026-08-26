---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Fall back to local resolution when `patchedDependencies` or `packageExtensions` are configured with a pnpr server, preserving patches and package extensions in the lockfile and installed packages.
