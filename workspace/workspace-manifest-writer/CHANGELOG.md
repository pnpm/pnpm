# @pnpm/workspace.manifest-writer

## 1100.0.2

### Patch Changes

- @pnpm/lockfile.types@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/workspace.workspace-manifest-reader@1100.0.1

## 1002.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- cb367b9: Remove deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies`.

### Minor Changes

- 7721d2e: `pnpm audit --fix` now adds the minimum patched versions to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` [#10263](https://github.com/pnpm/pnpm/issues/10263).

  When `minimumReleaseAge` is configured, security patches suggested by `pnpm audit` may be blocked because the patched versions are too new. Now, `pnpm audit --fix` automatically adds the minimum patched version for each vulnerability (e.g., `axios@0.21.2`) to `minimumReleaseAgeExclude`, so that `pnpm install` can install the security fix without waiting for it to mature.

- 76718b3: Added support for `allowBuilds`, which is a new field that can be used instead of `onlyBuiltDependencies` and `ignoredBuiltDependencies`. The new `allowBuilds` field in your `pnpm-workspace.yaml` uses a map of package matchers to explicitly allow (`true`) or disallow (`false`) script execution. This allows for a single, easy-to-manage source of truth for your build permissions.

  **Example Usage.** To explicitly allow all versions of `esbuild` to run scripts and prevent `core-js` from running them:

  ```yaml
  allowBuilds:
    esbuild: true
    core-js: false
  ```

  The example above achieves the same result as the previous configuration:

  ```yaml
  onlyBuiltDependencies:
    - esbuild
  ignoredBuiltDependencies:
    - core-js
  ```

  Related PR: [#10311](https://github.com/pnpm/pnpm/pull/10311)

- 121f64a: New option added: updatedOverrides.
- 075aa99: Add support for a global YAML config file named `config.yaml`.

  Now configurations are divided into 2 categories:

  - Registry and auth settings which can be stored in INI files such as global `rc` and local `.npmrc`.
  - pnpm-specific settings which can only be loaded from YAML files such as global `config.yaml` and local `pnpm-workspace.yaml`.

- 2b14c74: When pnpm updates the `pnpm-workspace.yaml`, comments, string formatting, and whitespace will be preserved.

### Patch Changes

- a1807b1: Prevent catalog entries from being removed by `cleanupUnusedCatalogs` when they are referenced only from workspace `overrides` in `pnpm-workspace.yaml`.
- 4f66fbe: Fix YAML formatting preservation in `pnpm-workspace.yaml` when running commands like `pnpm update`. Previously, quotes and other formatting were lost even when catalog values didn't change.

  Closes #10425

- Updated dependencies [c55c614]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [606f53e]
- Updated dependencies [491a84f]
- Updated dependencies [075aa99]
- Updated dependencies [8ed2c7d]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [2b14c74]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/workspace.workspace-manifest-reader@1001.0.0
  - @pnpm/config.parse-overrides@1002.0.0
  - @pnpm/object.key-sorting@1001.0.0
  - @pnpm/catalogs.types@1001.0.0
  - @pnpm/yaml.document-sync@1000.0.0

## 1001.0.3

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/lockfile.types@1002.0.2
  - @pnpm/workspace.read-manifest@1000.2.5

## 1001.0.2

### Patch Changes

- Updated dependencies [6365bc4]
  - @pnpm/constants@1001.3.1
  - @pnpm/workspace.read-manifest@1000.2.4

## 1001.0.1

### Patch Changes

- Updated dependencies [e792927]
  - @pnpm/types@1000.8.0
  - @pnpm/lockfile.types@1002.0.1
  - @pnpm/workspace.read-manifest@1000.2.3

## 1001.0.0

### Major Changes

- 9dbada8: Combine the logic of the `addCatalogs` function into the `updateWorkspaceManifest` function.

### Minor Changes

- 8747b4e: Added the `cleanupUnusedCatalogs` configuration. When set to `true`, pnpm will remove unused catalog entries during installation [#9793](https://github.com/pnpm/pnpm/pull/9793).

## 1000.2.3

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [f91922c]
  - @pnpm/constants@1001.3.0
  - @pnpm/lockfile.types@1002.0.0
  - @pnpm/workspace.read-manifest@1000.2.2

## 1000.2.2

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/lockfile.types@1001.1.0
  - @pnpm/constants@1001.2.0
  - @pnpm/workspace.read-manifest@1000.2.1

## 1000.2.1

### Patch Changes

- 95a9b82: Sort keys in `pnpm-workspace.yaml` with deep [#9701](https://github.com/pnpm/pnpm/pull/9701).

## 1000.2.0

### Minor Changes

- c8341cc: Added two new CLI options (`--save-catalog` and `--save-catalog-name=<name>`) to `pnpm add` to save new dependencies as catalog entries. `catalog:` or `catalog:<name>` will be added to `package.json` and the package specifier will be added to the `catalogs` or `catalog[<name>]` object in `pnpm-workspace.yaml` [#9425](https://github.com/pnpm/pnpm/issues/9425).

### Patch Changes

- Updated dependencies [c8341cc]
  - @pnpm/workspace.read-manifest@1000.2.0

## 1000.1.4

### Patch Changes

- Updated dependencies [c00360b]
  - @pnpm/object.key-sorting@1000.0.1
  - @pnpm/workspace.read-manifest@1000.1.5

## 1000.1.3

### Patch Changes

- 2bcb402: Sort keys in `pnpm-workspace.yaml` [#9453](https://github.com/pnpm/pnpm/pull/9453).

## 1000.1.2

### Patch Changes

- @pnpm/workspace.read-manifest@1000.1.4

## 1000.1.1

### Patch Changes

- ead11ad: Don't wrap lines in `pnpm-workspace.yaml`.
  - @pnpm/workspace.read-manifest@1000.1.3

## 1000.1.0

### Minor Changes

- 3a90ec1: `pnpm config delete --location=project` The setting in `pnpm-workspace.yaml` file will be deleted if no `.npmrc` file is present in the directory

### Patch Changes

- @pnpm/workspace.read-manifest@1000.1.2

## 1000.0.2

### Patch Changes

- @pnpm/workspace.read-manifest@1000.1.1

## 1000.0.1

### Patch Changes

- 23754c7: Fix the update of `pnpm-workspace.yaml` by the `pnpm approve-builds` command [#9168](https://github.com/pnpm/pnpm/issues/9168).

## 1000.0.0

### Major Changes

- 8fcc221: Initial release.
- 8fcc221: Initial release.

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
  - @pnpm/workspace.read-manifest@1000.1.0
