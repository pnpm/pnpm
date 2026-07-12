---
"./pnpm/npm/pnpm": patch
---

The `pnpm` wrapper's install script exits without error in the pnpm monorepo checkout, where the per-platform binary packages are not generated.
