# @pnpm/license-scanner

## 2.2.3

### Patch Changes

- @pnpm/directory-fetcher@7.0.6

## 2.2.2

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/lockfile-utils@9.0.0
  - @pnpm/directory-fetcher@7.0.5
  - @pnpm/store.cafs@2.0.8

## 2.2.1

### Patch Changes

- Updated dependencies [500363647]
  - @pnpm/directory-fetcher@7.0.4

## 2.2.0

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
  - @pnpm/types@9.4.0
  - @pnpm/store.cafs@2.0.7
  - @pnpm/directory-fetcher@7.0.3
  - @pnpm/lockfile-file@8.1.4
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/lockfile-walker@7.0.6
  - @pnpm/dependency-path@2.1.5
  - @pnpm/read-package-json@8.0.5

## 2.1.0

### Minor Changes

- fff7866f3: The `pnpm licenses list` command now accepts the `--filter` option to check the licenses of the dependencies of a subset of workspace projects [#5806](https://github.com/pnpm/pnpm/issues/5806).

## 2.0.22

### Patch Changes

- Updated dependencies [01bc58e2c]
  - @pnpm/store.cafs@2.0.6
  - @pnpm/directory-fetcher@7.0.2

## 2.0.21

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/package-is-installable@8.0.5
  - @pnpm/directory-fetcher@7.0.2
  - @pnpm/lockfile-file@8.1.3
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/lockfile-walker@7.0.5
  - @pnpm/dependency-path@2.1.4
  - @pnpm/read-package-json@8.0.4
  - @pnpm/store.cafs@2.0.5

## 2.0.20

### Patch Changes

- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5

## 2.0.19

### Patch Changes

- @pnpm/store.cafs@2.0.4
- @pnpm/directory-fetcher@7.0.1

## 2.0.18

### Patch Changes

- @pnpm/store.cafs@2.0.3
- @pnpm/directory-fetcher@7.0.0

## 2.0.17

### Patch Changes

- Updated dependencies [b3947185c]
  - @pnpm/store.cafs@2.0.2
  - @pnpm/directory-fetcher@7.0.0

## 2.0.16

### Patch Changes

- Updated dependencies [b548f2f43]
- Updated dependencies [4a1a9431d]
- Updated dependencies [d92070876]
  - @pnpm/store.cafs@2.0.1
  - @pnpm/directory-fetcher@7.0.0

## 2.0.15

### Patch Changes

- Updated dependencies [0fd9e6a6c]
- Updated dependencies [d57e4de6d]
- Updated dependencies [083bbf590]
- Updated dependencies [e9aa6f682]
  - @pnpm/store.cafs@2.0.0
  - @pnpm/directory-fetcher@6.1.0
  - @pnpm/lockfile-utils@8.0.4

## 2.0.14

### Patch Changes

- Updated dependencies [73f2b6826]
  - @pnpm/store.cafs@1.0.2
  - @pnpm/directory-fetcher@6.0.4

## 2.0.13

### Patch Changes

- Updated dependencies [fe1c5f48d]
  - @pnpm/store.cafs@1.0.1
  - @pnpm/directory-fetcher@6.0.4

## 2.0.12

### Patch Changes

- Updated dependencies [4bbf482d1]
  - @pnpm/store.cafs@1.0.0
  - @pnpm/directory-fetcher@6.0.4

## 2.0.11

### Patch Changes

- Updated dependencies [aa2ae8fe2]
- Updated dependencies [250f7e9fe]
- Updated dependencies [e958707b2]
  - @pnpm/types@9.2.0
  - @pnpm/cafs@7.0.5
  - @pnpm/package-is-installable@8.0.4
  - @pnpm/lockfile-file@8.1.2
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/lockfile-walker@7.0.4
  - @pnpm/dependency-path@2.1.3
  - @pnpm/read-package-json@8.0.3
  - @pnpm/directory-fetcher@6.0.4

## 2.0.10

### Patch Changes

- @pnpm/directory-fetcher@6.0.3

## 2.0.9

### Patch Changes

- Updated dependencies [b81cefdcd]
  - @pnpm/cafs@7.0.4
  - @pnpm/directory-fetcher@6.0.2

## 2.0.8

### Patch Changes

- c686768f0: `pnpm license ls` should work even when there is a patched git protocol dependency [#6595](https://github.com/pnpm/pnpm/issues/6595)
- Updated dependencies [e57e2d340]
  - @pnpm/cafs@7.0.3
  - @pnpm/directory-fetcher@6.0.2

## 2.0.7

### Patch Changes

- Updated dependencies [d9da627cd]
  - @pnpm/lockfile-utils@8.0.2
  - @pnpm/lockfile-file@8.1.1
  - @pnpm/error@5.0.2
  - @pnpm/package-is-installable@8.0.3
  - @pnpm/read-package-json@8.0.2
  - @pnpm/directory-fetcher@6.0.2

## 2.0.6

### Patch Changes

- 4b97f1f07: Don't use await in loops.
- Updated dependencies [d55b41a8b]
- Updated dependencies [614d5bd72]
  - @pnpm/cafs@7.0.2
  - @pnpm/directory-fetcher@6.0.1

## 2.0.5

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/lockfile-file@8.1.0
  - @pnpm/types@9.1.0
  - @pnpm/lockfile-utils@8.0.1
  - @pnpm/lockfile-walker@7.0.3
  - @pnpm/package-is-installable@8.0.2
  - @pnpm/dependency-path@2.1.2
  - @pnpm/read-package-json@8.0.1
  - @pnpm/cafs@7.0.1
  - @pnpm/error@5.0.1
  - @pnpm/directory-fetcher@6.0.1

## 2.0.4

### Patch Changes

- Updated dependencies [d58cdb962]
  - @pnpm/lockfile-utils@8.0.0

## 2.0.3

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/package-is-installable@8.0.1
  - @pnpm/dependency-path@2.1.1
  - @pnpm/lockfile-file@8.0.2
  - @pnpm/lockfile-utils@7.0.1
  - @pnpm/lockfile-walker@7.0.2

## 2.0.2

### Patch Changes

- Updated dependencies [72ba638e3]
  - @pnpm/lockfile-utils@7.0.0

## 2.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/lockfile-file@8.0.1
  - @pnpm/lockfile-utils@6.0.1
  - @pnpm/lockfile-walker@7.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [158d8cf22]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
- Updated dependencies [417c8ac59]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/lockfile-file@8.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/read-package-json@8.0.0
  - @pnpm/package-is-installable@8.0.0
  - @pnpm/directory-fetcher@6.0.0
  - @pnpm/lockfile-walker@7.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0
  - @pnpm/cafs@7.0.0

## 1.0.17

### Patch Changes

- Updated dependencies [787c43dcc]
  - @pnpm/lockfile-file@7.0.6
  - @pnpm/cafs@6.0.2
  - @pnpm/directory-fetcher@5.1.6

## 1.0.16

### Patch Changes

- 019e4f2de: Should not throw an error when local dependency use file protocol [#6115](https://github.com/pnpm/pnpm/issues/6115).

## 1.0.15

### Patch Changes

- Updated dependencies [ed946c73e]
  - @pnpm/lockfile-file@7.0.5

## 1.0.14

### Patch Changes

- @pnpm/directory-fetcher@5.1.5
- @pnpm/lockfile-utils@5.0.7
- @pnpm/cafs@6.0.1

## 1.0.13

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/lockfile-file@7.0.4
  - @pnpm/lockfile-utils@5.0.6
  - @pnpm/lockfile-walker@6.0.8

## 1.0.12

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2
  - @pnpm/lockfile-file@7.0.3
  - @pnpm/lockfile-utils@5.0.5
  - @pnpm/lockfile-walker@6.0.7

## 1.0.11

### Patch Changes

- Updated dependencies [98d6603f3]
- Updated dependencies [98d6603f3]
  - @pnpm/cafs@6.0.0
  - @pnpm/directory-fetcher@5.1.4

## 1.0.10

### Patch Changes

- Updated dependencies [1e6de89b6]
  - @pnpm/cafs@5.0.6
  - @pnpm/directory-fetcher@5.1.4

## 1.0.9

### Patch Changes

- Updated dependencies [9a68ebbae]
  - @pnpm/lockfile-file@7.0.2

## 1.0.8

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/lockfile-file@7.0.1
  - @pnpm/lockfile-utils@5.0.4
  - @pnpm/lockfile-walker@6.0.6

## 1.0.7

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/lockfile-file@7.0.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/cafs@5.0.5
  - @pnpm/error@4.0.1
  - @pnpm/lockfile-utils@5.0.3
  - @pnpm/lockfile-walker@6.0.5
  - @pnpm/package-is-installable@7.0.4
  - @pnpm/read-package-json@7.0.5
  - @pnpm/directory-fetcher@5.1.4

## 1.0.6

### Patch Changes

- 1d3995fe3: Add the 'description'-field to the licenses output [#5836](https://github.com/pnpm/pnpm/pull/5836).

## 1.0.5

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/package-is-installable@7.0.3
  - @pnpm/lockfile-file@6.0.5
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/lockfile-utils@5.0.2
  - @pnpm/lockfile-walker@6.0.4
  - @pnpm/dependency-path@1.0.1
  - @pnpm/read-package-json@7.0.4
  - @pnpm/cafs@5.0.4
  - @pnpm/directory-fetcher@5.1.3

## 1.0.4

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/lockfile-file@6.0.4
  - @pnpm/lockfile-utils@5.0.1
  - @pnpm/lockfile-walker@6.0.3

## 1.0.3

### Patch Changes

- 5464e1da6: `pnpm license list` should not fail if a license file is an executable [#5740](https://github.com/pnpm/pnpm/pull/5740).

## 1.0.2

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/lockfile-file@6.0.3
  - @pnpm/read-package-json@7.0.3
  - @pnpm/cafs@5.0.3
  - @pnpm/directory-fetcher@5.1.2

## 1.0.1

### Patch Changes

- @pnpm/directory-fetcher@5.1.1

## 1.0.0

### Major Changes

- d84a30a04: Added a new command `pnpm licenses list`, which displays the licenses of the packages [#2825](https://github.com/pnpm/pnpm/issues/2825)
