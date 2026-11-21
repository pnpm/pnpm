---
"@pnpm/releasing.versioning": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm version -r` updates explicit workspace dependency ranges that a release invalidates. `pnpm version` preserves edits that a `preversion` script makes to other manifest fields.
