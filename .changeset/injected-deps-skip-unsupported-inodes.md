---
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
---

`syncInjectedDepsAfterScripts` no longer fails with `ERR_PNPM_UNSUPPORTED_INODE_TYPE` when a workspace package contains an inode that is neither a file nor a directory, such as the FIFO 1Password's environments create for `.env`. Such an inode cannot be hardlinked into the injected copy, so it is skipped and the rest of the package still syncs [#13550](https://github.com/pnpm/pnpm/issues/13550).

`syncInjectedDepsAfterScripts` also no longer fails with `EEXIST` when a workspace package replaced a file with a directory of the same name since the injected copy was last synced.
