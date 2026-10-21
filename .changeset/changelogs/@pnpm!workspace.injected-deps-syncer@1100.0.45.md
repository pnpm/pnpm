## 1100.0.45

### Patch Changes

- A script listed in `syncInjectedDepsAfterScripts` now keeps a package's links to its own files as links in the injected copies, the way `pnpm install` writes them. It no longer fails when the package links to one directory twice or links back to a directory above the link. `pnpm install` with `deployAllFiles` no longer fails on such a link back either.

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- With `sharedWorkspaceLockfile: false`, a script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of the package it ran in. Before, those copies kept the files from the last install.

- A script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of a package that publishes from `publishConfig.directory`. Before, those copies kept the files from the last install.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.38
  - @pnpm/bins.remover@1100.0.29
  - @pnpm/bins.resolver@1100.0.18
  - @pnpm/fetching.directory-fetcher@1100.0.40
  - @pnpm/fs.indexed-pkg-importer@1100.0.35
  - @pnpm/installing.modules-yaml@1101.0.7
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.projects-reader@1101.1.5
