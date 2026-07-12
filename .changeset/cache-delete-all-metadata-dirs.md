---
"@pnpm/cache.commands": patch
"pnpm": patch
---

`pnpm cache delete` now removes a package's metadata from every metadata cache directory (`metadata`, `metadata-full`, and `metadata-full-filtered`), instead of only the one the current resolution mode reads. Previously a package cached under a different mode (e.g. `metadata-full-filtered`) was left behind. Closes pnpm/pnpm#12753.
