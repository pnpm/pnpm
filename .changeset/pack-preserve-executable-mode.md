---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pack` now preserves file executable permissions in the packed tarball when source files are executable on disk.
