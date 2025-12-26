---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fixed a bug ([#9759](https://github.com/pnpm/pnpm/issues/9759)) where `pnpm add` would incorrectly modify a catalog entry in `pnpm-workspace.yaml` to its exact version.
