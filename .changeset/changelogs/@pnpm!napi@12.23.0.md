## 12.23.0

### Minor Changes

- Added the `ignoreLockfileSettingsChecks` install option to the programmatic API. When it is enabled, pnpm does not compare the `settings` in `pnpm-lock.yaml` with the current configuration, so a frozen install no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` and other installs no longer re-resolve because of a mismatch.

- The `getDependents` options accept `excludePeerDependencies`, which leaves out the packages reached only as a resolved peer dependency, as `pnpm why --exclude-peers` does.

- The install options accept `lockfileFormat`, which applies the `lockfile.format` setting to the install. Set it to `"v13"` to write `pnpm-lock.yaml` in the experimental `lockfileVersion: '13.0'` format.

### Patch Changes

- `pnpm install` with `lockfile.format: v13` now writes `pnpm-lock.yaml` in the v13 format when it updates an existing v9 lockfile without resolving, for example after a dependency is removed. Such an install used to keep the v9 format.

- Fixed several mismatches between the `@pnpm/napi` API and its type declarations:

  - `install` with a single project no longer reads the `package.json` files of the other workspace projects from disk.
  - `install` with `ignorePackageManifest` accepts an empty `projects` list.
  - `install` rejects an unknown `nodeLinker`, `packageImportMethod`, or `networkConfig.localAddress` value with `ERR_PNPM_INVALID_OPTION`.
  - `resolveDependency` applies `proxyConfig` and `networkConfig`, returns the package's `resolution`, and keeps the `code` of pnpm errors.

- The `@pnpm/napi` addon is about 17% smaller on macOS and 20% smaller on Linux.
