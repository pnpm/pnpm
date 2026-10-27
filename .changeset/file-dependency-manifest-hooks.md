---
"pacquet": patch
---

`pnpm install --frozen-lockfile` no longer reports an outdated lockfile when a pnpmfile `readPackage` hook changes the dependencies of a `file:` directory dependency. A repeat `pnpm install` no longer resolves again when `ignoredOptionalDependencies` removes an optional dependency of such a package.
