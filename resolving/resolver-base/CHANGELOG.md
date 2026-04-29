# @pnpm/resolver-base

## 1100.1.0

### Minor Changes

- 72c1e05: Fix: different platform variants of the same runtime (e.g. `node@runtime:25.9.0` glibc vs. musl) no longer share a single global-virtual-store entry. The virtual store path now incorporates the selected variant's integrity, so installs with different `--os`/`--cpu`/`--libc` end up in separate directories and `pnpm add --libc=musl node@runtime:<v>` reliably fetches the musl binary even when the glibc variant is already cached.

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0

## 1006.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- facdd71: Adding `trustPolicyIgnoreAfter` allows you to ignore trust policy checks for packages published more than a specified time ago[#10352](https://github.com/pnpm/pnpm/issues/10352).
- 10bc391: Added a new setting: `trustPolicy`.
- 38b8e35: Support for custom resolvers and fetchers.
- 9d3f00b: Added support for `trustPolicyExclude` [#10164](https://github.com/pnpm/pnpm/issues/10164).

  You can now list one or more specific packages or versions that pnpm should allow to install, even if those packages don't satisfy the trust policy requirement. For example:

  ```yaml
  trustPolicy: no-downgrade
  trustPolicyExclude:
    - chokidar@4.0.3
    - webpack@4.47.0 || 5.102.1
  ```

### Patch Changes

- 9b0a460: Fixed a resolution bug that could cause `pnpm dedupe --check` to fail unexpectedly.

  When adding new dependencies to `package.json`, pnpm generally reuses existing versions in the `pnpm-lock.yaml` if they are satisfied by the version range specifier. There was an edge case where pnpm would instead resolve to a newly released version of a dependency. This is particularly problematic for `pnpm dedupe --check`, since a new version of a dependency published to the NPM registry could cause this check to suddenly fail. For details of this bug, see [#10626](https://github.com/pnpm/pnpm/issues/10626). This bug has been fixed.

  The fix necessitated a behavioral change: In some cases, pnpm was previously able to automatically dedupe a newly used dependency deep in the dependency graph without needing to run `pnpm dedupe`. This behavior was supported by the non-determinism that is now corrected. We believe fixing this non-determinism is more important than preserving an automatic dedupe heuristic that didn't handle all cases. The `pnpm dedupe` command can still be used to clean up dependencies that aren't automatically deduped on `pnpm install`.

- 50fbeca: Added `getNodeBinsForCurrentOS` to `@pnpm/constants` which returns a `Record<string, string>` with paths for `node`, `npm`, and `npx` within the Node.js package. This record is now used as `BinaryResolution.bin` (type widened from `string` to `string | Record<string, string>`) and as `manifest.bin` in the node resolver, so pnpm's bin-linker creates all three shims automatically when installing a Node.js runtime.
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/types@1001.0.0

## 1005.1.0

### Minor Changes

- 7c1382f: The npm resolver supports `publishedByExclude` now.

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0

## 1005.0.1

### Patch Changes

- Updated dependencies [e792927]
  - @pnpm/types@1000.8.0

## 1005.0.0

### Major Changes

- d1edf73: Rename Resolution to AtomicResolution. Add support for binary resolution.
- f91922c: Changed how the integrity of the node.js artifact is stored in the lockfile.

### Minor Changes

- 86b33e9: Added support for installing Bun runtime.

## 1004.1.0

### Minor Changes

- 1a07b8f: Added support for resolving and downloading the Node.js runtime specified in the [devEngines](https://github.com/openjs-foundation/package-metadata-interoperability-collab-space/issues/15) field of `package.json`.

  Usage example:

  ```json
  {
    "devEngines": {
      "runtime": {
        "name": "node",
        "version": "^24.4.0",
        "onFail": "download"
      }
    }
  }
  ```

  When running `pnpm install`, pnpm will resolve Node.js to the latest version that satisfies the specified range and install it as a dependency of the project. As a result, when running scripts, the locally installed Node.js version will be used.

  Unlike the existing options, `useNodeVersion` and `executionEnv.nodeVersion`, this new field supports version ranges, which are locked to exact versions during installation. The resolved version is stored in the pnpm lockfile, along with an integrity checksum for future validation of the Node.js content's validity.

  Related PR: [#9755](https://github.com/pnpm/pnpm/pull/9755).

### Patch Changes

- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0

## 1004.0.0

### Major Changes

- 2721291: Create different resolver result types which provide more information.
- 6acf819: Remove the blanket variant from the `Resolution` type, making it stricter and more useful.

## 1003.0.1

### Patch Changes

- Updated dependencies [5ec7255]
  - @pnpm/types@1000.6.0

## 1003.0.0

### Major Changes

- 8a9f3a4: `pref` renamed to `bareSpecifier`.
- 5b73df1: Renamed `normalizedPref` to `specifiers`.

### Minor Changes

- 9c3dd03: **Added support for installing JSR packages.** You can now install JSR packages using the following syntax:

  ```
  pnpm add jsr:<pkg_name>
  ```

  or with a version range:

  ```
  pnpm add jsr:<pkg_name>@<range>
  ```

  For example, running:

  ```
  pnpm add jsr:@foo/bar
  ```

  will add the following entry to your `package.json`:

  ```json
  {
    "dependencies": {
      "@foo/bar": "jsr:^0.1.2"
    }
  }
  ```

  When publishing, this entry will be transformed into a format compatible with npm, older versions of Yarn, and previous pnpm versions:

  ```json
  {
    "dependencies": {
      "@foo/bar": "npm:@jsr/foo__bar@^0.1.2"
    }
  }
  ```

  Related issue: [#8941](https://github.com/pnpm/pnpm/issues/8941).

  Note: The `@jsr` scope defaults to <https://npm.jsr.io/> if the `@jsr:registry` setting is not defined.

### Patch Changes

- Updated dependencies [5b73df1]
  - @pnpm/types@1000.5.0

## 1002.0.0

### Major Changes

- 81f441c: `updateToLatest` replaced with `update` field.

## 1001.0.0

### Major Changes

- 72cff38: The resolving function now takes a `registries` object, so it finds the required registry itself instead of receiving it from package requester.

### Patch Changes

- Updated dependencies [750ae7d]
  - @pnpm/types@1000.4.0

## 1000.2.1

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0

## 1000.2.0

### Minor Changes

- 3d52365: The `@pnpm/npm-resolver` package can now return `workspace` in the `resolvedVia` field of its results. This will be the case if the resolved package was requested through the `workspace:` protocol or if the wanted dependency's name and specifier match a package in the workspace. Previously, the `resolvedVia` field was always set to `local-filesystem` for workspace packages.

## 1000.1.4

### Patch Changes

- Updated dependencies [a5e4965]
  - @pnpm/types@1000.2.1

## 1000.1.3

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0

## 1000.1.2

### Patch Changes

- Updated dependencies [b562deb]
  - @pnpm/types@1000.1.1

## 1000.1.1

### Patch Changes

- Updated dependencies [9591a18]
  - @pnpm/types@1000.1.0

## 1000.1.0

### Minor Changes

- 6483b64: A new setting, `inject-workspace-packages`, has been added to allow hard-linking all local workspace dependencies instead of symlinking them. Previously, this behavior was achievable via the [`dependenciesMeta[].injected`](https://pnpm.io/package_json#dependenciesmetainjected) setting, which remains supported [#8836](https://github.com/pnpm/pnpm/pull/8836).

## 13.0.4

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0

## 13.0.3

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0

## 13.0.2

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/types@12.0.0

## 13.0.1

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0

## 13.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0

## 12.0.2

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1

## 12.0.1

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0

## 12.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- b13d2dc: It is now possible to install only a subdirectory from a Git repository.

  For example, `pnpm add github:user/repo#path:packages/foo` will add a dependency from the `packages/foo` subdirectory.

  This new parameter may be combined with other supported parameters separated by `&`. For instance, the next command will install the same package from the `dev` branch: `pnpm add github:user/repo#dev&path:packages/bar`.

  Related issue: [#4765](https://github.com/pnpm/pnpm/issues/4765).
  Related PR: [#7487](https://github.com/pnpm/pnpm/pull/7487).

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [43cdd87]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0

## 11.1.0

### Minor Changes

- 31054a63e: Running `pnpm update -r --latest` will no longer downgrade prerelease dependencies [#7436](https://github.com/pnpm/pnpm/issues/7436).

## 11.0.2

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2

## 11.0.1

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1

## 11.0.0

### Major Changes

- 4c2450208: (Important) Tarball resolutions in `pnpm-lock.yaml` will no longer contain a `registry` field. This field has been unused for a long time. This change should not cause any issues besides backward compatible modifications to the lockfile [#7262](https://github.com/pnpm/pnpm/pull/7262).

## 10.0.4

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0

## 10.0.3

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0

## 10.0.2

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0

## 10.0.1

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0

## 10.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/types@9.0.0

## 9.2.0

### Minor Changes

- 029143cff: Version selectors may have weights optionally.

### Patch Changes

- 029143cff: When resolving dependencies, prefer versions that are already used in the root of the project. This is important to minimize the number of packages that will be nested during hoisting [#6054](https://github.com/pnpm/pnpm/pull/6054).

## 9.1.5

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0

## 9.1.4

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0

## 9.1.3

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0

## 9.1.2

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0

## 9.1.1

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0

## 9.1.0

### Minor Changes

- 23984abd1: Add hook for adding custom fetchers.

## 9.0.6

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0

## 9.0.5

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0

## 9.0.4

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0

## 9.0.3

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0

## 9.0.2

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0

## 9.0.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1

## 9.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0

## 8.1.6

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0

## 8.1.5

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0

## 8.1.4

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0

## 8.1.3

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1

## 8.1.2

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0

## 8.1.1

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0

## 8.1.0

### Minor Changes

- 4ab87844a: New optional property added to `WantedDependency`: `injected`.

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0

## 8.0.4

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0

## 8.0.3

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0

## 8.0.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0

## 8.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0

## 8.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/types@7.0.0

## 7.1.1

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0

## 7.1.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

## 7.0.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1

## 7.0.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0

## 7.0.3

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0

## 7.0.2

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 7.0.1

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0

## 7.0.1-alpha.0

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
