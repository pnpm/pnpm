# pnpm

## 5.0.0-alpha.6

### Major Changes

- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
- Updated dependencies [471149e6]
- Updated dependencies [9fbb74ec]
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.4
  - @pnpm/plugin-commands-installation@2.0.0-alpha.6
  - @pnpm/plugin-commands-store@2.0.0-alpha.4
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/plugin-commands-outdated@1.0.10-alpha.2
  - @pnpm/plugin-commands-import@1.0.12-alpha.6
  - @pnpm/plugin-commands-server@1.0.11-alpha.4
  - @pnpm/cli-meta@1.0.0-alpha.0
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/default-reporter@7.2.5-alpha.1
  - @pnpm/plugin-commands-audit@1.0.9-alpha.1
  - @pnpm/plugin-commands-listing@1.0.10-alpha.1
  - @pnpm/plugin-commands-publishing@1.0.12-alpha.3
  - @pnpm/plugin-commands-script-runners@1.0.9-alpha.2
  - @pnpm/filter-workspace-packages@2.0.15-alpha.1

## 5.0.0-alpha.5

### Patch Changes

- @pnpm/plugin-commands-import@1.0.12-alpha.5
- @pnpm/plugin-commands-installation@1.2.4-alpha.5

## 5.0.0-alpha.4

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.3
  - @pnpm/plugin-commands-store@2.0.0-alpha.3
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/plugin-commands-audit@1.0.9-alpha.0
  - @pnpm/plugin-commands-import@1.0.12-alpha.4
  - @pnpm/plugin-commands-installation@1.2.4-alpha.4
  - @pnpm/plugin-commands-listing@1.0.10-alpha.0
  - @pnpm/plugin-commands-outdated@1.0.10-alpha.1
  - @pnpm/plugin-commands-server@1.0.11-alpha.3
  - @pnpm/plugin-commands-publishing@1.0.12-alpha.2
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/default-reporter@7.2.5-alpha.0
  - @pnpm/plugin-commands-script-runners@1.0.9-alpha.1
  - @pnpm/filter-workspace-packages@2.0.15-alpha.0

## 5.0.0-alpha.3

### Minor Changes

- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- 26c34c4f3: Print a meaningful error on unsupported Node.js versions.
- Updated dependencies [7300eba86]
- Updated dependencies [f453a5f46]
  - @pnpm/plugin-commands-script-runners@1.1.0-alpha.0
  - @pnpm/plugin-commands-installation@2.0.0-alpha.3
  - @pnpm/plugin-commands-publishing@1.0.12-alpha.1
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.2
  - @pnpm/plugin-commands-store@1.0.11-alpha.2
  - @pnpm/plugin-commands-server@1.0.11-alpha.2
  - @pnpm/plugin-commands-import@1.0.11-alpha.3
  - @pnpm/plugin-commands-outdated@1.0.10-alpha.0

## 5.0.0-alpha.2

### Major Changes

- 9e2a5b827: `pnpm r` is not an alias of `pnpm remove`.

### Patch Changes

- Updated dependencies [4063f1bee]
- Updated dependencies [9e2a5b827]
  - @pnpm/plugin-commands-publishing@2.0.0-alpha.0
  - @pnpm/plugin-commands-installation@2.0.0-alpha.2
  - @pnpm/plugin-commands-import@1.0.11-alpha.2

## 5.0.0-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/plugin-commands-installation@1.3.0-alpha.1
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.1
  - @pnpm/plugin-commands-store@1.0.11-alpha.1
  - @pnpm/plugin-commands-import@1.0.11-alpha.1
  - @pnpm/plugin-commands-server@1.0.11-alpha.1

## 5.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/plugin-commands-store@2.0.0-alpha.0
  - @pnpm/plugin-commands-installation@1.2.4-alpha.0
  - @pnpm/plugin-commands-server@1.0.11-alpha.0
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.0
  - @pnpm/plugin-commands-import@1.0.11-alpha.0
  - @pnpm/plugin-commands-listing@1.0.9-alpha.0
  - @pnpm/plugin-commands-outdated@1.0.9-alpha.0

## 4.14.2

### Patch Changes

- f8d6a07fe: Print a meaningful error on unsupported Node.js versions.
- Updated dependencies [c80d4ba3c]
  - @pnpm/plugin-commands-script-runners@1.1.0
  - @pnpm/plugin-commands-import@1.0.11
  - @pnpm/plugin-commands-installation@1.2.4
  - @pnpm/plugin-commands-publishing@1.0.12
  - @pnpm/plugin-commands-rebuild@1.0.11
  - @pnpm/plugin-commands-listing@1.0.9
  - @pnpm/plugin-commands-outdated@1.0.9

## 4.14.1

### Patch Changes

- 907c63a48: Update symlink-dir to v4.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/plugin-commands-outdated@1.0.9
  - @pnpm/plugin-commands-publishing@1.0.11
  - @pnpm/plugin-commands-server@1.0.10
  - @pnpm/plugin-commands-store@1.0.10
  - @pnpm/plugin-commands-installation@1.2.3
  - @pnpm/plugin-commands-listing@1.0.9
  - @pnpm/plugin-commands-rebuild@1.0.10
  - @pnpm/plugin-commands-script-runners@1.0.8
  - @pnpm/default-reporter@7.2.4
  - @pnpm/plugin-commands-import@1.0.10
  - @pnpm/plugin-commands-audit@1.0.8
  - @pnpm/filter-workspace-packages@2.0.14
  - @pnpm/cli-utils@0.4.4
