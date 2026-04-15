# @pnpm/config.config-writer

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/workspace.workspace-manifest-writer@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- 7721d2e: `pnpm audit --fix` now adds the minimum patched versions to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` [#10263](https://github.com/pnpm/pnpm/issues/10263).

  When `minimumReleaseAge` is configured, security patches suggested by `pnpm audit` may be blocked because the patched versions are too new. Now, `pnpm audit --fix` automatically adds the minimum patched version for each vulnerability (e.g., `axios@0.21.2`) to `minimumReleaseAgeExclude`, so that `pnpm install` can install the security fix without waiting for it to mature.

- 121f64a: New option added: updatedOverrides.

### Patch Changes

- Updated dependencies [7721d2e]
- Updated dependencies [a1807b1]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [121f64a]
- Updated dependencies [075aa99]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [4f66fbe]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [2b14c74]
  - @pnpm/workspace.workspace-manifest-writer@1002.0.0
  - @pnpm/types@1001.0.0

## 1000.0.14

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/read-project-manifest@1001.1.4
  - @pnpm/workspace.manifest-writer@1001.0.3

## 1000.0.13

### Patch Changes

- @pnpm/read-project-manifest@1001.1.3
- @pnpm/workspace.manifest-writer@1001.0.2

## 1000.0.12

### Patch Changes

- @pnpm/workspace.manifest-writer@1001.0.2
- @pnpm/read-project-manifest@1001.1.2

## 1000.0.11

### Patch Changes

- Updated dependencies [e792927]
  - @pnpm/types@1000.8.0
  - @pnpm/read-project-manifest@1001.1.1
  - @pnpm/workspace.manifest-writer@1001.0.1

## 1000.0.10

### Patch Changes

- Updated dependencies [9dbada8]
- Updated dependencies [8747b4e]
  - @pnpm/workspace.manifest-writer@1001.0.0

## 1000.0.9

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
  - @pnpm/read-project-manifest@1001.1.0
  - @pnpm/workspace.manifest-writer@1000.2.3

## 1000.0.8

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/read-project-manifest@1001.0.0
  - @pnpm/workspace.manifest-writer@1000.2.2

## 1000.0.7

### Patch Changes

- Updated dependencies [95a9b82]
  - @pnpm/workspace.manifest-writer@1000.2.1

## 1000.0.6

### Patch Changes

- Updated dependencies [c8341cc]
  - @pnpm/workspace.manifest-writer@1000.2.0

## 1000.0.5

### Patch Changes

- Updated dependencies [5ec7255]
  - @pnpm/types@1000.6.0
  - @pnpm/workspace.manifest-writer@1000.1.4
  - @pnpm/read-project-manifest@1000.0.11

## 1000.0.4

### Patch Changes

- Updated dependencies [2bcb402]
  - @pnpm/workspace.manifest-writer@1000.1.3

## 1000.0.3

### Patch Changes

- Updated dependencies [5b73df1]
  - @pnpm/types@1000.5.0
  - @pnpm/read-project-manifest@1000.0.10
  - @pnpm/workspace.manifest-writer@1000.1.2

## 1000.0.2

### Patch Changes

- 17b7e9f: The patch file path saved by the pnpm `patch-commit` and `patch-remove` commands should be a relative path [#9403](https://github.com/pnpm/pnpm/pull/9403).

## 1000.0.1

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [ead11ad]
  - @pnpm/types@1000.4.0
  - @pnpm/workspace.manifest-writer@1000.1.1
  - @pnpm/read-project-manifest@1000.0.9

## 1000.0.0

### Major Changes

- 5a9e34f: Initial release.

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [3a90ec1]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0
  - @pnpm/workspace.manifest-writer@1000.1.0
  - @pnpm/read-project-manifest@1000.0.8
