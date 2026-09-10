---
"pacquet": patch
---

`pnpm install` now runs a dependency's build scripts again when its side-effects cache entry has no files to restore. Such builds were skipped and nothing was put in their place, so a script whose whole effect lands outside its own package directory, such as a git-hook installer, never took effect [#14717](https://github.com/pnpm/pnpm/issues/14717).
