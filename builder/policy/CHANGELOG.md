# @pnpm/builder.policy

## 1000.1.3

### Patch Changes

- 14bceb1: Require trusted package identity before package-name `onlyBuiltDependencies` (and `allowBuilds`) entries can approve lifecycle scripts for git, git-hosted tarball, direct tarball, and local directory artifacts. To approve one of those artifacts explicitly, use its peer-suffix-free lockfile depPath as the key. Lockfile entries are now rejected when a registry-style dependency path (`name@semver`) is backed by a git, directory, or git-hosted tarball resolution (`ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`), so the dependency path is a reliable artifact identity by the time scripts can run.
- Updated dependencies [14bceb1]
  - @pnpm/types@1001.3.1
  - @pnpm/dependency-path@1001.1.11
  - @pnpm/config.version-policy@1000.0.7

## 1000.1.2

### Patch Changes

- @pnpm/config.version-policy@1000.0.6

## 1000.1.1

### Patch Changes

- Updated dependencies [d75628a]
  - @pnpm/types@1001.3.0
  - @pnpm/config.version-policy@1000.0.5

## 1000.1.0

### Minor Changes

- 512f188: Git dependencies with build scripts should respect the `dangerouslyAllowAllBuilds` settings [#10376](https://github.com/pnpm/pnpm/issues/10376).

## 1000.0.4

### Patch Changes

- Updated dependencies [59a81aa]
  - @pnpm/types@1001.2.0
  - @pnpm/config.version-policy@1000.0.4

## 1000.0.3

### Patch Changes

- Updated dependencies [9b05bdd]
  - @pnpm/types@1001.1.0
  - @pnpm/config.version-policy@1000.0.3

## 1000.0.2

### Patch Changes

- Updated dependencies [c206765]
  - @pnpm/types@1001.0.1
  - @pnpm/config.version-policy@1000.0.2

## 1000.0.1

### Patch Changes

- Updated dependencies [5847af4]
- Updated dependencies [68ad086]
- Updated dependencies [5847af4]
  - @pnpm/types@1001.0.0
  - @pnpm/config.version-policy@1000.0.1

## 1000.0.0

### Major Changes

- dee39ec: Sync version with pnpm CLI.

### Minor Changes

- dee39ec: You can now allow specific versions of dependencies to run postinstall scripts. `onlyBuiltDependencies` now accepts package names with lists of trusted versions. For example:

  ```yaml
  onlyBuiltDependencies:
    - nx@21.6.4 || 21.6.5
    - esbuild@0.25.1
  ```

  Related PR: [#10104](https://github.com/pnpm/pnpm/pull/10104).

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/config.version-policy@1000.0.0
