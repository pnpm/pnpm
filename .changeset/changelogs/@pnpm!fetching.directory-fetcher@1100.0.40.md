## 1100.0.40

### Patch Changes

- A script listed in `syncInjectedDepsAfterScripts` now keeps a package's links to its own files as links in the injected copies, the way `pnpm install` writes them. It no longer fails when the package links to one directory twice or links back to a directory above the link. `pnpm install` with `deployAllFiles` no longer fails on such a link back either.

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.21
  - @pnpm/fetching.fetcher-base@1100.2.13
  - @pnpm/fs.packlist@1100.0.9
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
