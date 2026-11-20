---
"@pnpm/building.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm approve-builds` now shows the exact selector to use when a bare package name fails to identify a pending `file:` dependency [#18237](https://github.com/pnpm/pnpm/issues/18237).
