---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` when a pnpmfile `readPackage` hook or `packageExtensions` changes the dependencies or peer dependencies of a local directory dependency the install holds no workspace project for: a `file:` dependency, an injected `workspace:../dir` dependency outside the workspace patterns, or an injected workspace package left out by `--filter`. It reported an outdated lockfile right after the install that wrote it [#17447](https://github.com/pnpm/pnpm/issues/17447) [#17473](https://github.com/pnpm/pnpm/issues/17473).

A repeat `pnpm install` now takes the fast path when `ignoredOptionalDependencies` removes an optional dependency of a `file:` directory dependency. It used to resolve again on every run.
