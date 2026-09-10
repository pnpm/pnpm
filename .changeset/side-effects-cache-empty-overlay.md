---
"pacquet": patch
---

`pnpm install` now runs a dependency's build scripts again when its side-effects cache entry has no files to restore. Such builds were skipped and nothing was put in their place, so a script whose whole effect lands outside its own package directory, such as a git-hook installer, never took effect. The same applies to an artifact from the shared side-effects cache, and pnpm no longer publishes such empty artifacts [#14717](https://github.com/pnpm/pnpm/issues/14717).
