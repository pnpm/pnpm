---
"pnpm": patch
"pacquet": patch
---

`pnpm outdated --json` and `pnpm outdated --format=json` now write reporter warnings to `stderr`. Warnings previously appeared on `stdout` before the JSON output.
