---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

A lockfile entry whose resolution is unchanged no longer loses its recorded `deprecated` marker when a registry serves the package's metadata inconsistently — re-resolving to the same version keeps the deprecation instead of silently dropping the line [#13846](https://github.com/pnpm/pnpm/issues/13846).
