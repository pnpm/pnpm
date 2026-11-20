---
"pacquet": minor
---

`pnpm pack` and `pnpm publish` now add a workspace-root `COPYING` file to a workspace package that has no license file. A package's own `COPYING` file now counts as its license, so the workspace-root `LICENSE` is no longer added next to it.
