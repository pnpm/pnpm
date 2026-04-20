# @pnpm/fetching.binary-fetcher

## 1100.0.2

### Patch Changes

- @pnpm/fetching.fetcher-base@1100.0.2
- @pnpm/worker@1100.0.2

## 1100.0.1

### Patch Changes

- @pnpm/fetching.fetcher-base@1100.0.1
- @pnpm/worker@1100.0.1

## 1003.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- 96704a1: Renamed `rawConfig` to `authConfig` on the `Config` interface. This field now only contains auth/registry data from `.npmrc` files. Non-auth settings are no longer written to it.

  Added `nodeDownloadMirrors` setting to configure custom Node.js download mirrors in `pnpm-workspace.yaml`:

  ```yaml
  nodeDownloadMirrors:
    release: https://my-mirror.example.com/download/release/
    nightly: https://my-mirror.example.com/download/nightly/
  ```

  Replaced `rawConfig: object` with `userAgent?: string` in lifecycle hook options. Removed unused `rawConfig` from fetcher and prepare-package options.

  Removed support for the npm `init-module` setting. Custom init scripts via `.pnpm-init.js` are no longer executed by `pnpm init`.

### Patch Changes

- 3bf5e21: Runtime dependencies (node, bun, deno) are now added to the store with a package.json file.
- 260899d: Fix path traversal vulnerability in binary fetcher ZIP extraction

  - Validate ZIP entry paths before extraction to prevent writing files outside target directory
  - Validate BinaryResolution.prefix (basename) to prevent directory escape via crafted prefix
  - Both attack vectors now throw `ERR_PNPM_PATH_TRAVERSAL` error

- 50fbeca: fix: preserve bundled `node_modules` from Node.js Windows zip so that npm/npx shims are created correctly on Windows.

  The Windows Node.js distribution places npm inside a root-level `node_modules/` directory of the zip archive. `addFilesFromDir` was skipping root-level `node_modules` (to avoid treating a package's own npm dependencies as part of its content), which caused the bundled npm to be missing after installation. This prevented `pnpm env use` from creating the npm and npx shims on Windows.

  Added an `includeNodeModules` option to `addFilesFromDir` and set it to `true` in the binary fetcher so that the complete Node.js distribution, including its bundled npm, is preserved.

- Updated dependencies [e2e0a32]
- Updated dependencies [7cec347]
- Updated dependencies [491a84f]
- Updated dependencies [50fbeca]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [bb8baa7]
- Updated dependencies [ee9fe58]
- Updated dependencies [7d2fd48]
- Updated dependencies [56a59df]
- Updated dependencies [780af09]
- Updated dependencies [6c480a4]
- Updated dependencies [4893853]
- Updated dependencies [b7f0f21]
- Updated dependencies [831f574]
- Updated dependencies [98a0410]
  - @pnpm/worker@1001.0.0
  - @pnpm/fetching.types@1001.0.0
  - @pnpm/fetching.fetcher-base@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/store.index@1000.0.0

## 1002.0.0

### Patch Changes

- Updated dependencies [8993f68]
  - @pnpm/worker@1000.3.0
  - @pnpm/fetcher-base@1001.0.2

## 1001.0.0

### Patch Changes

- Updated dependencies [06d2160]
  - @pnpm/worker@1000.2.0

## 1000.0.3

### Patch Changes

- @pnpm/error@1000.0.5
- @pnpm/worker@1000.1.13

## 1000.0.2

### Patch Changes

- @pnpm/fetcher-base@1001.0.1
- @pnpm/worker@1000.1.12

## 1000.0.1

### Patch Changes

- 2b0d35f: `@pnpm/worker` should always be a peer dependency.

## 1000.0.0

### Major Changes

- d1edf73: Added support for binary fetcher.

### Patch Changes

- Updated dependencies [d1edf73]
  - @pnpm/fetcher-base@1001.0.0
  - @pnpm/error@1000.0.4
  - @pnpm/worker@1000.1.11
