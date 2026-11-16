---
"pacquet": patch
---

`verifyDepsBeforeRun` no longer reports the dependencies as out of date when `runtimeOnFail` stops pnpm from installing the runtime that `devEngines.runtime` or `engines.runtime` declares [#18155](https://github.com/pnpm/pnpm/issues/18155).

Commands that install a single project of a workspace, such as `pnpm add -w`, now apply `runtimeOnFail` to the other workspace projects too. They no longer install a runtime that `runtimeOnFail` excludes.
