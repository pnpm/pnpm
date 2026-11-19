---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
---

Windows command shims preserve existing PATH entries when the `prependToPath` option is used.
