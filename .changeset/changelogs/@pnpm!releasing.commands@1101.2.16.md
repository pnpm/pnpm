## 1101.2.16

### Patch Changes

- Fixed `pnpm deploy` for projects that depend on an external directory through `link:`.

- `pnpm deploy` now installs a workspace package from its publish directory only when the package's `package.json` sets `publishConfig.directory` to that directory. A publish directory that only the lockfile records is ignored.

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- `pnpm pack --json` now reports the `package.json` that goes into the tarball, with `publishConfig` and the other publish-time changes applied. Run `pnpm pack --dry-run --json` to preview it without writing a tarball. A dry run still runs `prepack` and `prepare`, unless `--ignore-scripts` is passed.

- Packed workspace dependencies now use the dependency's `publishConfig.name`.

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- `pnpm stage` uses the current package's publish registry when listing, viewing, approving, rejecting, or downloading staged versions. To use a one-time password at a non-default registry selected by the package, pass `--registry`.

- `pnpm pack` and `pnpm publish` now reject `publishConfig.directory` values that use an absolute path or `..` segments outside the package directory.

- `pnpm version -r` updates explicit workspace dependency ranges that a release invalidates. `pnpm version` preserves edits that a `preversion` script makes to other manifest fields.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.22
  - @pnpm/building.policy@1100.1.12
  - @pnpm/cli.common-cli-options-help@1100.0.4
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.pick-registry-for-package@1101.0.7
  - @pnpm/config.reader@1102.4.0
  - @pnpm/deps.path@1101.0.11
  - @pnpm/engine.runtime.commands@1101.1.16
  - @pnpm/engine.runtime.node-resolver@1101.3.15
  - @pnpm/error@1100.2.3
  - @pnpm/exec.lifecycle@1100.1.31
  - @pnpm/exec.pnpm-cli-runner@1100.0.9
  - @pnpm/fetching.directory-fetcher@1100.0.46
  - @pnpm/fetching.types@1100.0.4
  - @pnpm/fs.indexed-pkg-importer@1100.0.41
  - @pnpm/fs.packlist@1100.0.15
  - @pnpm/installing.client@1100.3.23
  - @pnpm/installing.commands@1101.4.13
  - @pnpm/installing.deps-resolver@1102.3.0
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.peer-edges@1100.0.7
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/network.auth-header@1101.1.20
  - @pnpm/network.fetch@1100.1.27
  - @pnpm/network.web-auth@1101.6.4
  - @pnpm/releasing.exportable-manifest@1100.3.13
  - @pnpm/releasing.versioning@1100.3.11
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/resolving.registry.types@1100.2.7
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
  - @pnpm/workspace.projects-filter@1100.0.56
  - @pnpm/workspace.projects-graph@1100.0.50
  - @pnpm/workspace.projects-sorter@1101.0.6
  - @pnpm/workspace.task-scheduler@1100.0.9
  - @pnpm/workspace.workspace-manifest-writer@1100.2.13
