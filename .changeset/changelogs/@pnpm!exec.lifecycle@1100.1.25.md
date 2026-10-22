## 1100.1.25

### Patch Changes

- Fixed the README usage examples to match these packages' current APIs.

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.38
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/exec.npm-lifecycle@1100.0.6
  - @pnpm/fetching.directory-fetcher@1100.0.40
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.task-scheduler@1100.0.5
