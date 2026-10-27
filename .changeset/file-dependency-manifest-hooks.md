---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` when a pnpmfile `readPackage` hook changes the dependencies of a `file:` directory dependency. It reported an outdated lockfile right after the install that wrote it.

A repeat `pnpm install` now takes the fast path when `ignoredOptionalDependencies` removes an optional dependency of a `file:` directory dependency. It used to resolve again on every run.
