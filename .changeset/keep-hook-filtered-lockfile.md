---
"pnpm": patch
---

Fixed `pnpm add` and `pnpm update` dropping lockfile entries for projects missing from a sparse checkout when an `updateConfig` hook sets a project filter [#18465](https://github.com/pnpm/pnpm/issues/18465).
