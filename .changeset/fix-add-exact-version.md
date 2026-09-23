---
"@pnpm/pkg-manifest.utils": patch
"pnpm": patch
"pacquet": patch
---

The `add` command honors exact versions when requested rather than inheriting range operators from existing manifest entries.
