## 12.10.1

This release fixes `pnpm install` failures after an `overrides` change and on a filtered frozen install with `catalogPrune`. It also fixes several bugs in the experimental `nodeLinker.type: loaded`, which now keeps its generated files in `node_modules`.

### Patch Changes

- With `nodeLinker.type: loaded`, pnpm now writes its generated files to `node_modules`, which projects already ignore in git. The store manifest and loader are `node_modules/.pnpm/.store-manifest.json` and `node_modules/.pnpm/.store-loader.mjs`. Bin shims are in `node_modules/.bin`.

  Earlier versions wrote `.pnpm-store.json` and `.pnpm-store-loader.mjs` to the project root, and a `.pnpm` directory to the root and to each workspace package. Delete them after reinstalling.

- With `nodeLinker.type: loaded`, packages that ship their own `node_modules` directory, such as `npm` with its bundled dependencies, now load from the store. Before, one such package in the install stopped every Node.js process from starting.

- With `nodeLinker.type: loaded`, scripts can now run a Node.js runtime installed through `devEngines.runtime`. Before, every script that called `node` re-ran its own shim until it failed with "Argument list too long".

- With `nodeLinker.type: loaded`, Node.js processes start faster. In a project with 13,000 stored files, the startup overhead per process dropped from 67 ms to 18 ms.

- `pnpm install` no longer fails with `ERR_PNPM_NO_MATCHING_VERSION` after a change to `overrides` when the lockfile resolves an optional peer dependency to an npm alias of another package [#16654](https://github.com/pnpm/pnpm/issues/16654).

- A frozen install with `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records. Before, `pnpm install --frozen-lockfile --filter` failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when some workspace projects were missing from disk [#16638](https://github.com/pnpm/pnpm/issues/16638).

- `pnpm install --fix-lockfile` no longer removes the `deprecated` and `hasBin` fields from lockfile entries [#6600](https://github.com/pnpm/pnpm/issues/6600).

- With `enableGlobalVirtualStore`, an install that updates `node_modules` now repairs a package in the global virtual store that an interrupted install left without some of its dependency links or package files. Before, such an install kept the incomplete package if the project's `node_modules` already recorded it [#16642](https://github.com/pnpm/pnpm/issues/16642).

- `pnpm install` now skips the Cargo and Python projects inside a nested directory that has its own `pnpm-workspace.yaml` or `.git` directory, such as a git worktree of the same workspace or a separate clone.

- The `Request took` warning for package metadata now starts timing when pnpm sends the request. Before, it also counted the time the request waited for a free request slot, so large installs printed it for requests the registry answered quickly.
