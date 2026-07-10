---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm stage list` now stops paginating after a fail-safe cap of 1000 pages, so a misbehaving registry cannot keep the command looping forever.
