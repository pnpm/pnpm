---
"@pnpm/building.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm approve-builds <name>` now requires the exact selector shown by `pnpm ignored-builds` when the pending dependency is a file, git, or tarball artifact. A bare package name only approves registry dependencies [#18237](https://github.com/pnpm/pnpm/issues/18237).
