## 1100.1.15

### Patch Changes

- With `injectWorkspacePackages` and per-project lockfiles, `pnpm install` now updates a project's lockfile when a local tarball that an injected workspace project depends on is replaced. The project's lockfile kept the old version and integrity before.

- `pnpm install` now resolves a local directory again after its `package.json` changes when an override or a package extension adds it to the dependencies of a registry package. `pnpm install --frozen-lockfile` fails on a lockfile that still records the old dependencies of such a directory [#17486](https://github.com/pnpm/pnpm/issues/17486).

- Updated dependencies:
  - @pnpm/installing.context@1101.0.14
  - @pnpm/installing.deps-resolver@1102.2.11
  - @pnpm/lockfile.utils@1102.1.9
