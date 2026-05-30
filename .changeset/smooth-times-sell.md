---
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/deps.inspection.outdated": patch
---

`pnpm update -i` choices list now includes a `Provenance` column when `trustPolicy` is set to `no-downgrade`.
