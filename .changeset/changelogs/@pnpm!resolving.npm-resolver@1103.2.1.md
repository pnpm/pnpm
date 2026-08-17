## 1103.2.1

### Patch Changes

- `pnpm add` no longer re-resolves the dependency graph when `pnpm-lock.yaml` already holds a version satisfying the request — promoting a transitive dependency to a direct one, or adding to a second workspace package what a first one already depends on, now only saves the dependency in `package.json` and records its importer entry. A satisfying locked version is necessary but not sufficient: the install still falls back to a full resolution for a dist tag, an alias, a `workspace:`/`catalog:`/git/tarball specifier, `--save-peer`, an overridden package, a `catalogMode` other than `manual`, and — under `resolutionMode: time-based` or `lowest-direct`, which resolve a direct dependency to the low end of its range — a range several locked versions satisfy.

- `resolutionMode` is no longer ignored when `minimumReleaseAge` is in effect. `lowest-direct` and `time-based` pick the lowest satisfying version of a direct dependency again; previously any active release-age cutoff — including the built-in default — silently forced the highest, so `resolutionMode` only worked when `minimumReleaseAge: 0` was set explicitly [#13752](https://github.com/pnpm/pnpm/issues/13752).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.0
  - @pnpm/error@1100.1.2
  - @pnpm/pkg-manifest.utils@1100.4.0
  - @pnpm/resolving.jsr-specifier-parser@1100.0.5
  - @pnpm/store.cafs@1100.1.19
  - @pnpm/store.index@1100.2.4
