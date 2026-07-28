---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.modules-cleaner": patch
"pnpm": patch
"pacquet": patch
---

Speed up installs after safe override changes by reusing unambiguous compatible dependency resolutions, pruning obsolete dependencies, applying independent replacements and removals together, and handling parent-scoped `"-"` overrides without full lockfile resolution.
