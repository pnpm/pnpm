---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` when a pnpmfile `readPackage` hook changes the dependencies of a `file:` directory dependency, or the peers of an injected workspace package in a filtered install. It reported an outdated lockfile right after the install that wrote it [#17447](https://github.com/pnpm/pnpm/issues/17447).

A repeat `pnpm install` now takes the fast path when `ignoredOptionalDependencies` removes an optional dependency of a `file:` directory dependency. It used to resolve again on every run.
