---
"pacquet": minor
---

Added the `sideEffectsCacheExclude` setting for packages whose build output depends on the environment, such as native addons that read `JAVA_HOME`. pnpm builds the listed packages in every project and never restores or saves their builds through the side-effects cache. Under the global virtual store, each project gets its own copy of these packages. Entries use the same `name`, `@scope/*`, and `name@version` patterns as `minimumReleaseAgeExclude` [#5271](https://github.com/pnpm/pnpm/issues/5271).
