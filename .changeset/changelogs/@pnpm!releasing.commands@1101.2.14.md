## 1101.2.14

### Patch Changes

- `pnpm deploy` now runs the build scripts of a workspace project that it installs as a `file:` dependency, without a machine-specific `allowBuilds` key. An `allowBuilds` entry that denies the project still applies [pnpm/pnpm#17740](https://github.com/pnpm/pnpm/issues/17740).

- `pnpm deploy` now binds optional peers declared only in `peerDependenciesMeta` for linked workspace packages. The deployed manifest and lockfile also retain these peers when filtering dependency groups.

- `pnpm deploy` no longer fails when the deployed project depends on a workspace package that sets `publishConfig.directory`. pnpm installs that package from its publish directory and runs its build scripts [pnpm/pnpm#17771](https://github.com/pnpm/pnpm/pull/17771).

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).

- `pnpm deploy` preserves an injected workspace package's resolved dependencies when they differ from the workspace project's own dependencies. This also applies to packages with `publishConfig.directory` [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- Updated dependencies:
  - @pnpm/building.policy@1100.1.10
  - @pnpm/cli.utils@1101.0.35
  - @pnpm/config.reader@1102.3.9
  - @pnpm/deps.path@1101.0.9
  - @pnpm/engine.runtime.commands@1101.1.14
  - @pnpm/engine.runtime.node-resolver@1101.3.13
  - @pnpm/exec.lifecycle@1100.1.29
  - @pnpm/fetching.directory-fetcher@1100.0.44
  - @pnpm/fs.indexed-pkg-importer@1100.0.39
  - @pnpm/fs.packlist@1100.0.13
  - @pnpm/installing.client@1100.3.21
  - @pnpm/installing.commands@1101.4.11
  - @pnpm/installing.deps-resolver@1102.2.13
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.peer-edges@1100.0.5
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/network.fetch@1100.1.25
  - @pnpm/releasing.exportable-manifest@1100.3.11
  - @pnpm/releasing.versioning@1100.3.9
  - @pnpm/resolving.npm-resolver@1104.2.10
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/workspace.projects-filter@1100.0.54
  - @pnpm/workspace.projects-graph@1100.0.48
  - @pnpm/workspace.workspace-manifest-writer@1100.2.11
