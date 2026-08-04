---
"pacquet": patch
---

`pnpm install --frozen-lockfile` no longer re-imports a varying subset of packages on every repeat install of an unchanged project [#13316](https://github.com/pnpm/pnpm/issues/13316). The global-virtual-store directory of a package that takes part in a dependency cycle was derived from an order that changed from run to run, so those packages landed on a fresh slot each time; it is now derived deterministically and matches the directory pnpm itself computes.
