## 1103.0.12

### Patch Changes

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.38
  - @pnpm/building.pkg-requires-build@1100.0.21
  - @pnpm/building.policy@1100.1.8
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.reader@1102.3.6
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.graph-hasher@1100.3.8
  - @pnpm/deps.path@1101.0.7
  - @pnpm/exec.lifecycle@1100.1.25
  - @pnpm/fs.symlink-dependency@1100.0.25
  - @pnpm/installing.context@1101.0.12
  - @pnpm/installing.deps-restorer@1103.2.6
  - @pnpm/installing.modules-yaml@1101.0.7
  - @pnpm/lockfile.settings-checker@1100.2.14
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/lockfile.walker@1100.0.28
  - @pnpm/patching.config@1100.1.10
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/store.cafs@1100.3.8
  - @pnpm/store.connection-manager@1101.3.5
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/store.index@1101.0.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.task-scheduler@1100.0.5
