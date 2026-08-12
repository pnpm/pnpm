---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm deploy --no-optional` no longer writes a lockfile whose snapshots reference optional dependencies that the deploy excluded.
