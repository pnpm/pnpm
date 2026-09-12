---
"@pnpm/pkg-manifest.utils": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

`pnpm add` no longer re-resolves the dependency graph when `pnpm-lock.yaml` already holds a version satisfying the request — promoting a transitive dependency to a direct one, or adding to a second workspace package what a first one already depends on, now only saves the dependency in `package.json` and records its importer entry. A satisfying locked version is necessary but not sufficient: the install still falls back to a full resolution for a dist tag, an alias, a `workspace:`/`catalog:`/git/tarball specifier, `--save-peer`, an overridden package, a `catalogMode` other than `manual`, and — under `resolutionMode: time-based` or `lowest-direct`, which resolve a direct dependency to the low end of its range — a range several locked versions satisfy.
