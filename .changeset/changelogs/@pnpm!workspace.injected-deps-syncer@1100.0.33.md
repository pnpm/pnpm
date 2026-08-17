## 1100.0.33

### Patch Changes

- `syncInjectedDepsAfterScripts` no longer fails with `ERR_PNPM_UNSUPPORTED_INODE_TYPE` when a workspace package contains an inode that is neither a file nor a directory, such as the FIFO 1Password's environments create for `.env`. Such an inode cannot be hardlinked into the injected copy, so it is skipped and the rest of the package still syncs [#13550](https://github.com/pnpm/pnpm/issues/13550).

  `syncInjectedDepsAfterScripts` also no longer fails with `EEXIST` when a workspace package replaced a file with a directory of the same name since the injected copy was last synced.

- `syncInjectedDepsAfterScripts` no longer fails with `ENOTDIR` when a workspace package replaced a directory with a file of the same name and the injected copy still held that directory's contents.

- `syncInjectedDepsAfterScripts` now removes the bin link of a bin the script dropped. Previously only new bins were linked, so a build step that stopped declaring one left its shim behind, pointing at a command that was no longer there.

- `syncInjectedDepsAfterScripts` now identifies a file by its device as well as its inode number. An inode number is only unique within one filesystem, so on its own it could match an unrelated file on another device and leave that path stale in the injected copy.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.27
  - @pnpm/bins.remover@1100.0.20
  - @pnpm/error@1100.1.2
  - @pnpm/fetching.directory-fetcher@1100.0.29
  - @pnpm/pkg-manifest.reader@1100.0.16
  - @pnpm/workspace.projects-reader@1101.0.23
