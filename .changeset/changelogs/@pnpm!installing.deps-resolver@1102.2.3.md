## 1102.2.3

### Patch Changes

- `pnpm add` and `pnpm install` now support installing bzip2 compressed tarballs [https://github.com/pnpm/pnpm/issues/6761](https://github.com/pnpm/pnpm/issues/6761).

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm install --engine-strict` now respects `engines` relaxed by `readPackage` hooks in `.pnpmfile.cjs` [pnpm/pnpm#15482](https://github.com/pnpm/pnpm/issues/15482).

- `pnpm add` and `pnpm remove` no longer move unrelated transitive dependencies to other versions. Adding a package and then removing it now leaves `pnpm-lock.yaml` unchanged. Before, the dependencies of auto-installed peers and `npm:` aliased subdependencies could move to a newer version that was already in the lockfile [#11859](https://github.com/pnpm/pnpm/issues/11859).

- On Windows, pnpm now fails within about a second when it cannot move a `node_modules` directory installed by another package manager because a file in it is in use. The error names the directory and suggests stopping the process that uses it. pnpm used to retry for a minute and then print a raw `EPERM` stack trace [#7505](https://github.com/pnpm/pnpm/issues/7505).

- `pnpm update` no longer warns "Skip adding ... to the default catalog" for a dependency that already uses `catalog:` [#13715](https://github.com/pnpm/pnpm/issues/13715).

- pnpm now installs a dependency that a package also declares as an optional peer dependency, for example `lightningcss` in some vite builds. The dependency was missing from `node_modules`, so the package failed to import it [#8912](https://github.com/pnpm/pnpm/issues/8912).

- pnpm no longer writes a package's legacy array-form `engines`, such as `["node >= 0.8"]`, to the lockfile. It was recorded as an object keyed by index, such as `{'0': node >= 0.8}` [#4518](https://github.com/pnpm/pnpm/issues/4518).

- Removing an entry from `overrides` now re-resolves the packages it targeted. A version the override had locked is no longer kept just because the declared range still accepts it [#4587](https://github.com/pnpm/pnpm/issues/4587).

- Secondary dependencies now prefer the version resolved by the local project's direct dependencies over versions from sibling workspace projects [pnpm/pnpm#7191](https://github.com/pnpm/pnpm/issues/7191).

- `packageExtensions` ranged selectors (such as `@<X` or `@*`) no longer match dependencies that ship without a `package.json` [pnpm/pnpm#15007](https://github.com/pnpm/pnpm/issues/15007).

- `packageExtensions` and `overrides` entries with a ranged selector (such as `@<X` or `@*`) no longer match a local directory dependency that has no `package.json` [pnpm/pnpm#15007](https://github.com/pnpm/pnpm/issues/15007).

- `pnpm update <name>` now updates a peer dependency that pnpm installed automatically [#10486](https://github.com/pnpm/pnpm/issues/10486).

- Added a `--peer` flag to `pnpm update` to update ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

- A lockfile entry whose resolution is unchanged now keeps its recorded `deprecated` message [#5772](https://github.com/pnpm/pnpm/issues/5772).

- Support SemVer build metadata in workspace dependency resolution

- `pnpm install` now re-resolves a workspace project's auto-installed peer dependency when another workspace project changes its specifier for that package to one that excludes the locked version but still overlaps the peer range. The peer then resolves to the version a fresh install would pick [#11800](https://github.com/pnpm/pnpm/issues/11800).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/fetching.pick-fetcher@1100.1.12
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/hooks.types@1101.0.4
  - @pnpm/lockfile.preferred-versions@1100.0.35
  - @pnpm/lockfile.pruner@1100.0.25
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/patching.config@1100.1.7
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
