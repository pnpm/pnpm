## 12.0.0

### Minor Changes

- `@pnpm/napi`'s `install` now honors the last two install options it accepted without acting on them:

  - `ignorePackageManifest: true` installs from `pnpm-lock.yaml` alone, ignoring the project manifests — pnpm's `pnpm fetch` semantics. Every importer the lockfile records is imported into the virtual store, and no post-import linking is performed: no importer symlinks, no `.bin` entries, no hoisting, and no project lifecycle scripts. It previously only skipped the manifest ↔ lockfile freshness check and otherwise linked a full `node_modules`.
  - `pnpmHomeDir` now places the default store at `<pnpmHomeDir>/store`, with the same same-volume fallback pnpm applies. An explicit `storeDir` — passed alongside it or set by a config source — still wins. It was previously ignored.

- Added an `allowUnusedPatches` install option. When `true`, a `patchedDependencies` entry that matches no installed package warns instead of failing the install with `ERR_PNPM_UNUSED_PATCH`.

- `readConfig` now returns `explicitSettings` — the camelCase names of settings the config cascade set explicitly — so hosts that layer the resolved config over their own defaults can forward only the values the user actually configured.

- `install` accepts `enableGlobalVirtualStore`, `globalVirtualStoreDir`, `packageExtensions` and `patchedDependencies`, and `readConfig` reports `enableGlobalVirtualStore`, `globalVirtualStoreDir`, `virtualStoreDir` and `effectiveVirtualStoreDir`. Hosts embedding the engine can now use the global virtual store, declare dependencies a package failed to declare, patch a package, and locate the virtual store instead of assuming `node_modules/.pnpm`.

- Added `readConfig(options)`: resolves the configuration the engine's own installs use — registries with their resolved `Authorization` headers, `authHeaderByUri`, proxy, TLS, network limits, store/cache directories, and install behavior settings from the `.npmrc` / `pnpm-workspace.yaml` cascade — so hosts that embed the engine no longer need a JavaScript config reader.

- Added the `trustLockfile` install option to skip verifying lockfile resolutions against current registry metadata.

- Added `pnpm licenses` command to the Rust pacquet port to list package licenses in a tabular or JSON format.

- Added `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps` to the Rust pnpm CLI and its N-API bindings. Slow registry metadata requests and tarball downloads now emit pnpm-compatible warnings without exposing URL credentials, query parameters, fragments, or control characters [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added a `returnListOfDepsRequiringBuild` install option. When it is set, `InstallResult.depsRequiringBuild` lists the dep path of every package whose files carry install scripts, whether or not the scripts were allowed to run, matching the TypeScript CLI's option of the same name. An install that computes no list, such as one served from the lockfile, leaves the field undefined.

### Patch Changes

- Fixed repeat installs ignoring changes in caller-supplied in-memory project manifests.

- Fixed `resolveDependency({ fullMetadata: true })` returning a manifest stripped down to the abbreviated npm field set. Registry-custom fields on the version object (such as Bit's `componentId`) are now preserved.
