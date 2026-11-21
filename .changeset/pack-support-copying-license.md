---
"pacquet": minor
---

`pnpm pack` and `pnpm publish` now add a workspace-root `COPYING` file to a workspace package that has no license file. A `COPYING` file at the package root now counts as the package's license, so the workspace-root `LICENSE` is no longer added next to it.
