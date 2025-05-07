# @pnpm/lockfile.verification

## 1001.1.6

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/resolver-base@1003.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/types@1000.5.0
  - @pnpm/lockfile.utils@1001.0.10
  - @pnpm/get-context@1001.0.13
  - @pnpm/lockfile.types@1001.0.7
  - @pnpm/dependency-path@1000.0.8
  - @pnpm/read-package-json@1000.0.8
  - @pnpm/crypto.hash@1000.1.1

## 1001.1.5

### Patch Changes

- Updated dependencies [81f441c]
  - @pnpm/resolver-base@1002.0.0
  - @pnpm/lockfile.utils@1001.0.9
  - @pnpm/get-context@1001.0.12
  - @pnpm/crypto.hash@1000.1.1

## 1001.1.4

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [72cff38]
  - @pnpm/types@1000.4.0
  - @pnpm/resolver-base@1001.0.0
  - @pnpm/lockfile.types@1001.0.6
  - @pnpm/lockfile.utils@1001.0.8
  - @pnpm/dependency-path@1000.0.7
  - @pnpm/get-context@1001.0.11
  - @pnpm/read-package-json@1000.0.7
  - @pnpm/crypto.hash@1000.1.1

## 1001.1.3

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0
  - @pnpm/lockfile.types@1001.0.5
  - @pnpm/lockfile.utils@1001.0.7
  - @pnpm/dependency-path@1000.0.6
  - @pnpm/get-context@1001.0.10
  - @pnpm/read-package-json@1000.0.6
  - @pnpm/resolver-base@1000.2.1
  - @pnpm/crypto.hash@1000.1.1

## 1001.1.2

### Patch Changes

- Updated dependencies [3d52365]
  - @pnpm/resolver-base@1000.2.0
  - @pnpm/get-context@1001.0.9
  - @pnpm/lockfile.utils@1001.0.6
  - @pnpm/crypto.hash@1000.1.1

## 1001.1.1

### Patch Changes

- @pnpm/crypto.hash@1000.1.1
- @pnpm/dependency-path@1000.0.5
- @pnpm/lockfile.utils@1001.0.5
- @pnpm/get-context@1001.0.8

## 1001.1.0

### Minor Changes

- daf47e9: Projects using a `file:` dependency on a local tarball file (i.e. `.tgz`, `.tar.gz`, `.tar`) will see a performance improvement during installation. Previously, using a `file:` dependency on a tarball caused the lockfile resolution step to always run. The lockfile will now be considered up-to-date if the tarball is unchanged.

### Patch Changes

- Updated dependencies [daf47e9]
- Updated dependencies [a5e4965]
  - @pnpm/crypto.hash@1000.1.0
  - @pnpm/types@1000.2.1
  - @pnpm/dependency-path@1000.0.4
  - @pnpm/lockfile.types@1001.0.4
  - @pnpm/lockfile.utils@1001.0.4
  - @pnpm/get-context@1001.0.7
  - @pnpm/read-package-json@1000.0.5
  - @pnpm/resolver-base@1000.1.4

## 1001.0.6

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0
  - @pnpm/lockfile.types@1001.0.3
  - @pnpm/lockfile.utils@1001.0.3
  - @pnpm/dependency-path@1000.0.3
  - @pnpm/get-context@1001.0.6
  - @pnpm/read-package-json@1000.0.4
  - @pnpm/resolver-base@1000.1.3

## 1001.0.5

### Patch Changes

- @pnpm/get-context@1001.0.5

## 1001.0.4

### Patch Changes

- Updated dependencies [b562deb]
  - @pnpm/types@1000.1.1
  - @pnpm/get-context@1001.0.4
  - @pnpm/lockfile.types@1001.0.2
  - @pnpm/lockfile.utils@1001.0.2
  - @pnpm/dependency-path@1000.0.2
  - @pnpm/read-package-json@1000.0.3
  - @pnpm/resolver-base@1000.1.2

## 1001.0.3

### Patch Changes

- Updated dependencies [9591a18]
  - @pnpm/types@1000.1.0
  - @pnpm/lockfile.types@1001.0.1
  - @pnpm/lockfile.utils@1001.0.1
  - @pnpm/dependency-path@1000.0.1
  - @pnpm/get-context@1001.0.3
  - @pnpm/read-package-json@1000.0.2
  - @pnpm/resolver-base@1000.1.1

## 1001.0.2

### Patch Changes

- @pnpm/get-context@1001.0.2

## 1001.0.1

### Patch Changes

- @pnpm/get-context@1001.0.1

## 1001.0.0

### Major Changes

- a76da0c: Removed lockfile conversion from v6 to v9. If you need to convert lockfile v6 to v9, use pnpm CLI v9.

### Patch Changes

- Updated dependencies [6483b64]
- Updated dependencies [a76da0c]
  - @pnpm/resolver-base@1000.1.0
  - @pnpm/lockfile.types@1001.0.0
  - @pnpm/get-context@1001.0.0
  - @pnpm/lockfile.utils@1001.0.0
  - @pnpm/read-package-json@1000.0.1

## 1.1.0

### Minor Changes

- 19d5b51: Export `linkedPackagesAreUpToDate` and `getWorkspacePackagesByDirectory`

### Patch Changes

- Updated dependencies [dcd2917]
- Updated dependencies [9ea8fa4]
- Updated dependencies [9ea8fa4]
- Updated dependencies [9ea8fa4]
- Updated dependencies [9ea8fa4]
- Updated dependencies [9ea8fa4]
- Updated dependencies [d55b259]
  - @pnpm/dependency-path@6.0.0
  - @pnpm/get-context@13.0.0
  - @pnpm/lockfile.utils@1.0.5
  - @pnpm/read-package-json@9.0.10

## 1.0.6

### Patch Changes

- Updated dependencies [f9a095c]
  - @pnpm/get-context@12.0.7
  - @pnpm/dependency-path@5.1.7
  - @pnpm/lockfile.utils@1.0.4

## 1.0.5

### Patch Changes

- @pnpm/get-context@12.0.6
- @pnpm/read-package-json@9.0.9

## 1.0.4

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0
  - @pnpm/lockfile.types@1.0.3
  - @pnpm/lockfile.utils@1.0.3
  - @pnpm/dependency-path@5.1.6
  - @pnpm/get-context@12.0.5
  - @pnpm/read-package-json@9.0.8
  - @pnpm/resolver-base@13.0.4

## 1.0.3

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/lockfile.types@1.0.2
  - @pnpm/lockfile.utils@1.0.2
  - @pnpm/dependency-path@5.1.5
  - @pnpm/get-context@12.0.4
  - @pnpm/read-package-json@9.0.7
  - @pnpm/resolver-base@13.0.3

## 1.0.2

### Patch Changes

- dc902fd: Don't crash when the lockfile doesn't have a project in it during verification.

## 1.0.1

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/lockfile.types@1.0.1
  - @pnpm/types@12.0.0
  - @pnpm/lockfile.utils@1.0.1
  - @pnpm/dependency-path@5.1.4
  - @pnpm/get-context@12.0.3
  - @pnpm/read-package-json@9.0.6
  - @pnpm/resolver-base@13.0.2

## 1.0.0

### Major Changes

- 2e3eae3: Initial release.

### Patch Changes

- Updated dependencies [c5ef9b0]
- Updated dependencies [797ef0f]
  - @pnpm/lockfile.utils@1.0.0
  - @pnpm/lockfile.types@1.0.0
  - @pnpm/get-context@12.0.2
