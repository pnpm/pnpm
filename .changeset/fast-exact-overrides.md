---
"pnpm": patch
---

Speed up installs after safe override changes by reusing unambiguous compatible dependency resolutions, pruning obsolete dependencies, applying independent replacements and removals together, and handling parent-scoped `"-"` overrides without full lockfile resolution.
