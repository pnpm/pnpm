# @pnpm/building.policy

## 1100.0.11

### Patch Changes

- Updated dependencies [25a829e]
- Updated dependencies [fbdc0eb]
  - @pnpm/config.version-policy@1100.1.6

## 1100.0.10

### Patch Changes

- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
  - @pnpm/types@1101.3.2
  - @pnpm/config.version-policy@1100.1.5
  - @pnpm/deps.path@1100.0.8

## 1100.0.9

### Patch Changes

- bf1b731: Require trusted package identity before package-name `allowBuilds` entries can approve lifecycle scripts for git, git-hosted tarball, direct tarball, and local directory artifacts. To approve one of those artifacts explicitly, use its peer-suffix-free lockfile depPath as the `allowBuilds` key. Lockfile verification now rejects lockfiles where a registry-style dependency path (`name@semver`) is backed by a git, directory, or git-hosted tarball resolution (`ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`), so the dependency path is a reliable artifact identity by the time scripts can run.
- Updated dependencies [bf1b731]
  - @pnpm/types@1101.3.1
  - @pnpm/config.version-policy@1100.1.4
  - @pnpm/deps.path@1100.0.7

## 1100.0.8

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/types@1101.3.0
  - @pnpm/config.version-policy@1100.1.3
  - @pnpm/deps.path@1100.0.6

## 1100.0.7

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0
  - @pnpm/config.version-policy@1100.1.2
  - @pnpm/deps.path@1100.0.5

## 1100.0.6

### Patch Changes

- Updated dependencies [64afc92]
  - @pnpm/types@1101.1.1
  - @pnpm/config.version-policy@1100.1.1
  - @pnpm/deps.path@1100.0.4

## 1100.0.5

### Patch Changes

- Updated dependencies [b6e2c8c]
  - @pnpm/config.version-policy@1100.1.0

## 1100.0.4

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0
  - @pnpm/config.version-policy@1100.0.3
  - @pnpm/deps.path@1100.0.3

## 1100.0.3

### Patch Changes

- ab6c42d: Treat `allowBuilds` as an install-state input and clear previously ignored builds when they are explicitly disallowed.

## 1100.0.2

### Patch Changes

- 184ce26: Fix the package name in README.md.
  - @pnpm/config.version-policy@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/config.version-policy@1100.0.1

## 1000.0.0

### Major Changes

- 2fccb03: Initial release
- cb367b9: Remove deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies`.
- 7354e6b: Initial release.

### Minor Changes

- 82f4610: Git dependencies with build scripts should respect the `dangerouslyAllowAllBuilds` settings [#10376](https://github.com/pnpm/pnpm/issues/10376).

### Patch Changes

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
  - @pnpm/config.version-policy@1000.0.1
