---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` when a pnpmfile `readPackage` hook or `packageExtensions` changes the dependencies or peer dependencies of a local directory dependency that is not a workspace project, such as a `file:` dependency or an injected `workspace:../dir` dependency outside the workspace patterns. It reported an outdated lockfile right after the install that wrote it [#17473](https://github.com/pnpm/pnpm/issues/17473).

A repeat `pnpm install` now takes the fast path when `ignoredOptionalDependencies` removes an optional dependency of a `file:` directory dependency. It used to resolve again on every run.
