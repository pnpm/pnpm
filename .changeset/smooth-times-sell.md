---
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/deps.inspection.outdated": patch
---

`pnpm update -i` choices list now includes a `provenance` column when `trustPolicy` is set to `no-downgrade`.
