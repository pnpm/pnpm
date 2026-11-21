---
"@pnpm/building.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm approve-builds` now requires the exact selector shown by `pnpm ignored-builds` for a dependency a bare package name cannot identify (a `file:`, git, or tarball artifact). A bare package name only approves default-registry dependencies [#18237](https://github.com/pnpm/pnpm/issues/18237).
