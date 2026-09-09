---
"pacquet": patch
---

`pnpm install` now runs a dependency's build scripts when the side-effects cache has no build output to restore in their place. Such a build was skipped and nothing was materialized for it, so a script whose whole effect lands outside its own package directory, such as a git-hook installer, never took effect [#14717](https://github.com/pnpm/pnpm/issues/14717).
