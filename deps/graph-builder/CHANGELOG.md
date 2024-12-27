# @pnpm/deps.graph-builder

## 1001.0.2

### Patch Changes

- Updated dependencies [516c4b3]
- Updated dependencies [4771813]
  - @pnpm/core-loggers@1000.1.0
  - @pnpm/modules-yaml@1000.1.0
  - @pnpm/package-is-installable@1000.0.2

## 1001.0.1

### Patch Changes

- Updated dependencies [3f0e4f0]
  - @pnpm/lockfile.fs@1001.1.0

## 1001.0.0

### Major Changes

- a76da0c: Removed lockfile conversion from v6 to v9. If you need to convert lockfile v6 to v9, use pnpm CLI v9.

### Patch Changes

- Updated dependencies [d2e83b0]
- Updated dependencies [6483b64]
- Updated dependencies [a76da0c]
  - @pnpm/constants@1001.0.0
  - @pnpm/store-controller-types@1000.1.0
  - @pnpm/lockfile.utils@1001.0.0
  - @pnpm/lockfile.fs@1001.0.0
  - @pnpm/package-is-installable@1000.0.1

## 2.0.6

### Patch Changes

- Updated dependencies [19d5b51]
- Updated dependencies [8108680]
- Updated dependencies [dcd2917]
- Updated dependencies [e476b07]
- Updated dependencies [d55b259]
- Updated dependencies [c4f5231]
  - @pnpm/constants@10.0.0
  - @pnpm/dependency-path@6.0.0
  - @pnpm/package-is-installable@9.0.12
  - @pnpm/lockfile.fs@1.0.6
  - @pnpm/lockfile.utils@1.0.5
  - @pnpm/store-controller-types@18.1.6

## 2.0.5

### Patch Changes

- @pnpm/package-is-installable@9.0.11
- @pnpm/dependency-path@5.1.7
- @pnpm/lockfile.fs@1.0.5
- @pnpm/lockfile.utils@1.0.4

## 2.0.4

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/constants@9.0.0
  - @pnpm/lockfile.fs@1.0.4
  - @pnpm/package-is-installable@9.0.10

## 2.0.3

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0
  - @pnpm/package-is-installable@9.0.9
  - @pnpm/lockfile.fs@1.0.3
  - @pnpm/lockfile.utils@1.0.3
  - @pnpm/core-loggers@10.0.7
  - @pnpm/dependency-path@5.1.6
  - @pnpm/modules-yaml@13.1.7
  - @pnpm/store-controller-types@18.1.6

## 2.0.2

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/package-is-installable@9.0.8
  - @pnpm/lockfile.fs@1.0.2
  - @pnpm/lockfile.utils@1.0.2
  - @pnpm/core-loggers@10.0.6
  - @pnpm/dependency-path@5.1.5
  - @pnpm/modules-yaml@13.1.6
  - @pnpm/store-controller-types@18.1.5

## 2.0.1

### Patch Changes

- Updated dependencies [33ba536]
  - @pnpm/package-is-installable@9.0.7

## 2.0.0

### Major Changes

- cb006df: Add ability to apply patch to all versions:
  If the key of `pnpm.patchedDependencies` is a package name without a version (e.g. `pkg`), pnpm will attempt to apply the patch to all versions of
  the package, failure will be skipped.
  If it is a package name and an exact version (e.g. `pkg@x.y.z`), pnpm will attempt to apply the patch to that exact version only, failure will
  cause pnpm to fail.

  If there's only one version of `pkg` installed, `pnpm patch pkg` and subsequent `pnpm patch-commit $edit_dir` will create an entry named `pkg` in
  `pnpm.patchedDependencies`. And pnpm will attempt to apply this patch to other versions of `pkg` in the future.

  If there's multiple versions of `pkg` installed, `pnpm patch pkg` will ask which version to edit and whether to attempt to apply the patch to all.
  If the user chooses to apply the patch to all, `pnpm patch-commit $edit_dir` would create a `pkg` entry in `pnpm.patchedDependencies`.
  If the user chooses not to apply the patch to all, `pnpm patch-commit $edit_dir` would create a `pkg@x.y.z` entry in `pnpm.patchedDependencies` with
  `x.y.z` being the version the user chose to edit.

  If the user runs `pnpm patch pkg@x.y.z` with `x.y.z` being the exact version of `pkg` that has been installed, `pnpm patch-commit $edit_dir` will always
  create a `pkg@x.y.z` entry in `pnpm.patchedDependencies`.

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/patching.config@1.0.0
  - @pnpm/patching.types@1.0.0
  - @pnpm/types@12.0.0
  - @pnpm/lockfile.fs@1.0.1
  - @pnpm/lockfile.utils@1.0.1
  - @pnpm/package-is-installable@9.0.6
  - @pnpm/core-loggers@10.0.5
  - @pnpm/dependency-path@5.1.4
  - @pnpm/modules-yaml@13.1.5
  - @pnpm/store-controller-types@18.1.4

## 1.1.9

### Patch Changes

- Updated dependencies [c5ef9b0]
- Updated dependencies [8055a30]
  - @pnpm/lockfile.utils@1.0.0
  - @pnpm/lockfile.fs@1.0.0

## 1.1.8

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0
  - @pnpm/package-is-installable@9.0.5
  - @pnpm/lockfile-file@9.1.3
  - @pnpm/lockfile-utils@11.0.4
  - @pnpm/core-loggers@10.0.4
  - @pnpm/dependency-path@5.1.3
  - @pnpm/modules-yaml@13.1.4
  - @pnpm/store-controller-types@18.1.3

## 1.1.7

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/store-controller-types@18.1.2
  - @pnpm/package-is-installable@9.0.4
  - @pnpm/lockfile-file@9.1.2
  - @pnpm/core-loggers@10.0.3
  - @pnpm/dependency-path@5.1.2
  - @pnpm/modules-yaml@13.1.3

## 1.1.6

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/package-is-installable@9.0.3
  - @pnpm/lockfile-file@9.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/core-loggers@10.0.2
  - @pnpm/dependency-path@5.1.1
  - @pnpm/modules-yaml@13.1.2
  - @pnpm/store-controller-types@18.1.1

## 1.1.5

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-file@9.1.0
  - @pnpm/lockfile-utils@11.0.1

## 1.1.4

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-controller-types@18.1.0

## 1.1.3

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0
  - @pnpm/package-is-installable@9.0.2
  - @pnpm/lockfile-file@9.0.6
  - @pnpm/core-loggers@10.0.1
  - @pnpm/modules-yaml@13.1.1
  - @pnpm/store-controller-types@18.0.1

## 1.1.2

### Patch Changes

- @pnpm/package-is-installable@9.0.1
- @pnpm/lockfile-file@9.0.5

## 1.1.1

### Patch Changes

- Updated dependencies [7a0536e]
  - @pnpm/lockfile-utils@10.1.1
  - @pnpm/lockfile-file@9.0.4

## 1.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/modules-yaml@13.1.0
  - @pnpm/lockfile-utils@10.1.0
  - @pnpm/lockfile-file@9.0.3

## 1.0.3

### Patch Changes

- Updated dependencies [c969f37]
  - @pnpm/lockfile-file@9.0.2

## 1.0.2

### Patch Changes

- Updated dependencies [2cbf7b7]
- Updated dependencies [6b6ca69]
  - @pnpm/lockfile-file@9.0.1

## 1.0.1

### Patch Changes

- b7d2ed4: The `engines.pnpm` field in the `package.json` files of dependencies should be ignored [#7965](https://github.com/pnpm/pnpm/issues/7965).

## 1.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- cdd8365: Package ID does not contain the registry domain.
- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [f67ad31]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/types@10.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/package-is-installable@9.0.0
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/modules-yaml@13.0.0
  - @pnpm/lockfile-file@9.0.0
  - @pnpm/core-loggers@10.0.0

## 0.2.8

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/store-controller-types@17.2.0
  - @pnpm/lockfile-utils@9.0.5

## 0.2.7

### Patch Changes

- Updated dependencies [d349bc3a2]
  - @pnpm/modules-yaml@12.1.7

## 0.2.6

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/lockfile-file@8.1.6
  - @pnpm/lockfile-utils@9.0.4
  - @pnpm/package-is-installable@8.1.2
  - @pnpm/core-loggers@9.0.6
  - @pnpm/dependency-path@2.1.7
  - @pnpm/modules-yaml@12.1.6
  - @pnpm/store-controller-types@17.1.4

## 0.2.5

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/lockfile-file@8.1.5
  - @pnpm/lockfile-utils@9.0.3
  - @pnpm/package-is-installable@8.1.1
  - @pnpm/core-loggers@9.0.5
  - @pnpm/dependency-path@2.1.6
  - @pnpm/modules-yaml@12.1.5
  - @pnpm/store-controller-types@17.1.3

## 0.2.4

### Patch Changes

- Updated dependencies [d5a176af7]
  - @pnpm/lockfile-utils@9.0.2

## 0.2.3

### Patch Changes

- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1

## 0.2.2

### Patch Changes

- fe1f0f734: Fixed a performance regression on running installation on a project with an up to date lockfile [#7297](https://github.com/pnpm/pnpm/issues/7297).
- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2

## 0.2.1

### Patch Changes

- Updated dependencies [4c2450208]
- Updated dependencies [7ea45afbe]
  - @pnpm/lockfile-utils@9.0.0
  - @pnpm/store-controller-types@17.1.1

## 0.2.0

### Minor Changes

- 43ce9e4a6: Support for multiple architectures when installing dependencies [#5965](https://github.com/pnpm/pnpm/issues/5965).

  You can now specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install. Use the `supportedArchitectures` field in `package.json` to define your preferences.

  For example, the following configuration tells pnpm to install optional dependencies for Windows x64:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32"],
        "cpu": ["x64"]
      }
    }
  }
  ```

  Whereas this configuration will have pnpm install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32", "darwin", "current"],
        "cpu": ["x64", "arm64"]
      }
    }
  }
  ```

  Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/package-is-installable@8.1.0
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-file@8.1.4
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/core-loggers@9.0.4
  - @pnpm/dependency-path@2.1.5
  - @pnpm/modules-yaml@12.1.4

## 0.1.5

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/package-is-installable@8.0.5
  - @pnpm/lockfile-file@8.1.3
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/core-loggers@9.0.3
  - @pnpm/dependency-path@2.1.4
  - @pnpm/modules-yaml@12.1.3
  - @pnpm/store-controller-types@17.0.1

## 0.1.4

### Patch Changes

- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5

## 0.1.3

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0

## 0.1.2

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0

## 0.1.1

### Patch Changes

- @pnpm/store-controller-types@16.0.1

## 0.1.0

### Minor Changes

- 494f87544: Breaking changes to the API.

### Patch Changes

- Updated dependencies [494f87544]
- Updated dependencies [e9aa6f682]
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/lockfile-utils@8.0.4

## 0.0.1

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/package-is-installable@8.0.4
  - @pnpm/lockfile-file@8.1.2
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/core-loggers@9.0.2
  - @pnpm/dependency-path@2.1.3
  - @pnpm/modules-yaml@12.1.2
  - @pnpm/store-controller-types@15.0.2
