## 11.28.6

### Patch Changes

- pnpm again accepts `httpProxy` and `httpsProxy` set to `null` or written with no value in `pnpm-workspace.yaml` and the global config file, and treats them as unset. Since 11.28.5 these values failed with `ERR_PNPM_INVALID_SETTING`.

- `pnpm install` no longer fails with `ERR_PNPM_NO_MATCHING_VERSION` after a change to `overrides` when the lockfile resolves an optional peer dependency to an npm alias of another package [#16654](https://github.com/pnpm/pnpm/issues/16654).

- A key under a registry URL in the `_auth` setting that does not start with `@` is now skipped with a warning, so a field a later pnpm version adds there no longer breaks this one. A key that holds an `authToken` object but lacks its `@`, such as `org` for `@org`, is still an error.

- `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records for workspace projects missing from disk. This affected `pnpm remove` and installs that leave the lockfile unchanged, such as frozen installs. Before, a following frozen install failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).

- `pnpm install` and `pnpm add --config` now apply `minimumReleaseAge` when they resolve a config dependency. A config dependency range resolves to the newest version that is old enough, so a later clean `pnpm install --frozen-lockfile` accepts the lockfile [#16660](https://github.com/pnpm/pnpm/issues/16660).

- `pnpm install --fix-lockfile` no longer removes the `deprecated` and `hasBin` fields from lockfile entries [#6600](https://github.com/pnpm/pnpm/issues/6600).

- `pnpm deploy` in a workspace with `injectWorkspacePackages` no longer crashes when a linked workspace package declares a peer dependency named after an `Object.prototype` member, such as `constructor`.

- `pnpm deploy` without `injectWorkspacePackages` now binds a peer dependency of a linked workspace package to the version its parent provides, as an injected install does. Previously the deploy failed with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` whenever other packages in the deployed graph depended on different versions of that peer [#16807](https://github.com/pnpm/pnpm/issues/16807).

- Escape codes in script output that moved the cursor or cleared the screen garbled pnpm's output. pnpm now drops them and keeps only color codes. Color codes are dropped too when pnpm's own output is not in color.

- A filtered install, such as `pnpm install --filter <pkg>`, now keeps the `pnpm-lock.yaml` entries of workspace projects that are missing from disk, as in a sparse checkout. Their importers, packages, and catalog entries stay even when the filter matches every project on disk [#16931](https://github.com/pnpm/pnpm/issues/16931).

- With `enableGlobalVirtualStore`, an install that updates `node_modules` now restores the dependency links of a package in the global virtual store that an interrupted install left without them. Before, such an install kept the incomplete package if the project's `node_modules` already recorded it [#16642](https://github.com/pnpm/pnpm/issues/16642).

- `pnpm install` hardlinks files from a group-writable or world-writable store again. Before this fix, pnpm copied every file from such a store into `node_modules` [#16677](https://github.com/pnpm/pnpm/issues/16677).

- Removing a `link:` dependency now also removes its symlink from `node_modules` when `nodeLinker` is `hoisted` [#16884](https://github.com/pnpm/pnpm/issues/16884).

- With `sharedWorkspaceLockfile: false`, an injected workspace package installed in the same run as its dependent now holds only the files its `files` field selects. The copy used to also hold other files of the project, such as `tsconfig.json` [#16683](https://github.com/pnpm/pnpm/issues/16683).

- A script listed in `syncInjectedDepsAfterScripts` no longer fails when it rewrites `package.json` while pnpm is copying its edits into the injected copies. The sync after the script now replaces a half-copied manifest.

- pnpm no longer drops a dependency that a package also declares as an optional peer dependency, for example `ajv` of `ajv-formats`. Since 11.28.0, in some dependency graphs the package got the dependency neither as a dependency nor as a peer, and failed to import it [#16901](https://github.com/pnpm/pnpm/issues/16901).

- `pnpm dedupe` now preserves a hoisted workspace package when another package uses a separately installed peer with the same name [pnpm/pnpm#16935](https://github.com/pnpm/pnpm/issues/16935).

- Sped up workspace discovery with subtree exclusions such as `!generated/**`.

- `pnpm publish` now uses an `NPM_ID_TOKEN` only if its `aud` claim matches the selected registry, such as `npm:registry.npmjs.org`. A token for another registry is skipped with a warning, and pnpm falls back to the configured credentials.

- pnpm now relinks the bins of a pnpm version it installed earlier for `packageManager` or `pnpm with`. A pnpm 12 version installed by a pnpm release older than 11.28.4 could keep a launcher that ran `node` on the native binary, so every command failed with `SyntaxError: Invalid or unexpected token` [#16646](https://github.com/pnpm/pnpm/issues/16646).

- Removal overrides such as `"debug>supports-color": "-"` now also apply to an optional peer that a package declares only in `peerDependenciesMeta` [#16681](https://github.com/pnpm/pnpm/issues/16681).

- Sped up frozen installs of unchanged injected workspace packages that use hardlinks.

- The warning about renamed package files with invalid names no longer prints control characters from those file names.

- `pnpm setup` no longer tries to install pnpm globally from npm's `bin` directory when pnpm was installed with npm. It failed with `Failed to create bin at <PNPM_HOME>/bin/pnpm` [#16855](https://github.com/pnpm/pnpm/issues/16855).

- When the registry rejects `pnpm stage publish`, the error message now starts with "Failed to stage package".

- An install that adds nothing to the store no longer changes the modification time of the store directory. CI caches that check modification times, such as GitLab's, no longer re-upload an unchanged store [#16929](https://github.com/pnpm/pnpm/issues/16929).

- pnpm now switches to the pnpm version a project pins even when it cannot read its configuration. Previously, a setting it rejected in the global config file, an `.npmrc` file, `_auth`, or `pnpm-workspace.yaml` stopped every command, `pnpm --version` included.

- Fixed `pnpm install` failing on Windows with `EPERM` on `rmdir node_modules/@scope` when several workspace packages under one scope are hoisted, for example with `nodeLinker: hoisted` [#16690](https://github.com/pnpm/pnpm/issues/16690).

- `pnpm install` and `pnpm dedupe` now link an optional peer to the workspace package that the workspace root depends on when the picked version matches it. Previously they installed the registry package with the same name and version [pnpm/pnpm#16706](https://github.com/pnpm/pnpm/issues/16706).
