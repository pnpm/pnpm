---
"@pnpm/exec.prepare-package": patch
"pnpm": patch
"pacquet": patch
---

Installing a git-hosted dependency that has to be built no longer fails when that dependency's own dependencies have build scripts nobody approved. pnpm skips those builds while preparing the dependency, as it does without `strictDepBuilds` [#9764](https://github.com/pnpm/pnpm/issues/9764).
