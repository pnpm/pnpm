## 12.13.0

This release adds the `pnpm profile` and `pnpm token` commands, the `storeIndexFormat: files` setting for stores shared over a network filesystem, and faster Python dependency resolution.

### Minor Changes

- Added the `pnpm profile` command to view and update registry user profile settings.

- Added `pnpm token` to list, create, and revoke authentication tokens on the registry.

- Added the `storeIndexFormat` setting. Set it to `files` to keep a store's package index as one file per package instead of the SQLite `index.db`, for a store on a network filesystem that several machines use at once. SQLite's locking does not work across machines, so concurrent installs on such a store could corrupt `index.db`.

- Added the `nodePackageMap` setting, which groups the package-map settings. `nodePackageMap.mode: write-only` writes `node_modules/.package-map.json` for other tools without making Node.js load it. `mode: load` also makes Node.js load the map, like `nodeExperimentalPackageMap: true`. `nodePackageMap.type` replaces `nodePackageMapType`. The old settings still work [#14937](https://github.com/pnpm/pnpm/issues/14937).

  ```yaml
  nodePackageMap:
    mode: write-only
    type: loose
  ```

### Patch Changes

#### Setting up pnpm

- `pnpm setup` now works when pnpm was installed with npm on macOS. It installed pnpm from npm's `bin` directory and left a `$PNPM_HOME/bin/pnpm` that could not run [#16855](https://github.com/pnpm/pnpm/issues/16855).

- pnpm runs again after `pnpm setup` when `globalShims` enables pnpm. Every command failed with "the global target of the pnpm shim points back at the shim" [#16874](https://github.com/pnpm/pnpm/issues/16874).

- A pnpm built for the MinGW target on Windows, such as the MSYS2 package, now installs the MinGW Rust toolchain that `rust-toolchain.toml` names. It used to install the MSVC toolchain.

#### Installing packages

- On macOS, `pnpm install` no longer fails with `ERR_PNPM_LOCKFILE_WRITE_FILE` when the project is on a virtiofs or SMB share. pnpm now falls back to a regular `fsync` when the file system rejects a full sync [#16846](https://github.com/pnpm/pnpm/issues/16846).

- On macOS, `pnpm install`, `pnpm patch-commit`, and other commands that load a pnpmfile no longer hang at random.

- `pnpm install` now puts the executables of a Python build backend's requirements on the `PATH` while the backend runs. Projects built with `uv_build` now install. Before, they failed with "uv-build was not properly installed".

- Removing a `link:` dependency now also removes its symlink from `node_modules` when `nodeLinker` is `hoisted` [#16884](https://github.com/pnpm/pnpm/issues/16884).

- `pnpm-lock.yaml` no longer records the tarball URL of a package installed from a scoped registry, such as one set by `@types:registry=`, when pnpm can rebuild the URL from that registry [#16866](https://github.com/pnpm/pnpm/issues/16866).

#### Packing, auditing, and pipelines

- `pnpm pack` and `pnpm publish` now pack only `package.json`, the README, the license, and the files named in `main` and `bin` when `files` is an empty array. They used to pack every file in the package [#16889](https://github.com/pnpm/pnpm/issues/16889).

- `pnpm audit` and `pnpm audit signatures` no longer check runtimes installed through `devEngines.runtime`, such as `node@runtime:24.21.0`, against the npm package of the same name. `pnpm audit signatures` reported these runtimes as having an invalid registry signature [#16869](https://github.com/pnpm/pnpm/issues/16869).

- `pnpm pipeline` now runs the root's tasks when `pnpm-workspace.yaml` lists only the root in `packages` or omits `packages`, as `pnpm -r run` does. A change to a root file in such a workspace no longer triggers a full-run warning. When a full run has no projects to run, `pnpm pipeline` now reports that no projects are selected [#16849](https://github.com/pnpm/pnpm/issues/16849).

#### Performance

- `pnpm install` resolves Python dependencies faster. With `--lockfile-only`, a project with 10 dependencies now resolves in about 4 seconds, down from about 40. When the index publishes a metadata file beside each wheel (PEP 658), as PyPI does, pnpm downloads only the wheels of the releases it selects.

- A repeat install against an existing lockfile no longer reads registry metadata for the dependencies it takes from that lockfile. On a warm store this removes thousands of metadata lookups from a repeat install of a large workspace.

- `pnpm install` with `enableModulesDir: false` now verifies the packages already in the store while the dependencies resolve, and checks a package's files in parallel. On a 5,100-package install with the store on a network filesystem, this removed a 19-second pause between resolution and fetching.
