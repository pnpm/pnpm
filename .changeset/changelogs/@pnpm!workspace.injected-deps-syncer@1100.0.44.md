## 1100.0.44

### Patch Changes

- With `sharedWorkspaceLockfile: false`, an injected workspace package installed in the same run as its dependent now holds only the files its `files` field selects. The copy used to also hold other files of the project, such as `tsconfig.json` [#16683](https://github.com/pnpm/pnpm/issues/16683).

- A script listed in `syncInjectedDepsAfterScripts` no longer fails when it rewrites `package.json` while pnpm is copying its edits into the injected copies. The sync after the script now replaces a half-copied manifest.

- Updated dependencies:
  - @pnpm/fetching.directory-fetcher@1100.0.39
  - @pnpm/workspace.projects-reader@1101.1.4
