# @pnpm/plugin-commands-licenses

## 4.1.9

### Patch Changes

- Updated dependencies [1b03682]
- Updated dependencies [9b5b869]
  - @pnpm/config@21.6.0
  - @pnpm/command@5.0.2
  - @pnpm/cli-utils@3.1.3
  - @pnpm/lockfile-file@9.1.2
  - @pnpm/license-scanner@3.1.7

## 4.1.8

### Patch Changes

- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [04b8363]
  - @pnpm/config@21.5.0
  - @pnpm/cli-utils@3.1.2
  - @pnpm/lockfile-file@9.1.1
  - @pnpm/license-scanner@3.1.6

## 4.1.7

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/lockfile-file@9.1.0
  - @pnpm/config@21.4.0
  - @pnpm/license-scanner@3.1.5
  - @pnpm/cli-utils@3.1.1

## 4.1.6

### Patch Changes

- Updated dependencies [b7ca13f]
- Updated dependencies [b7ca13f]
  - @pnpm/cli-utils@3.1.0
  - @pnpm/config@21.3.0

## 4.1.5

### Patch Changes

- @pnpm/config@21.2.3
- @pnpm/license-scanner@3.1.4
- @pnpm/cli-utils@3.0.7

## 4.1.4

### Patch Changes

- 34bc8f4: Details in the `pnpm licenses` output are misplaced [#8071](https://github.com/pnpm/pnpm/pull/8071).
  - @pnpm/cli-utils@3.0.6
  - @pnpm/config@21.2.2
  - @pnpm/lockfile-file@9.0.6
  - @pnpm/license-scanner@3.1.3

## 4.1.3

### Patch Changes

- Updated dependencies [a7aef51]
- Updated dependencies [37538f5]
  - @pnpm/error@6.0.1
  - @pnpm/command@5.0.1
  - @pnpm/cli-utils@3.0.5
  - @pnpm/config@21.2.1
  - @pnpm/lockfile-file@9.0.5
  - @pnpm/license-scanner@3.1.2
  - @pnpm/store-path@9.0.1

## 4.1.2

### Patch Changes

- @pnpm/cli-utils@3.0.4

## 4.1.1

### Patch Changes

- 21de734: Details in the `pnpm outdated` output are wrapped correctly [#8037](https://github.com/pnpm/pnpm/pull/8037).
  - @pnpm/lockfile-file@9.0.4
  - @pnpm/license-scanner@3.1.1

## 4.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/license-scanner@3.1.0
  - @pnpm/config@21.2.0
  - @pnpm/lockfile-file@9.0.3
  - @pnpm/cli-utils@3.0.3

## 4.0.4

### Patch Changes

- Updated dependencies [c969f37]
  - @pnpm/lockfile-file@9.0.2
  - @pnpm/license-scanner@3.0.2

## 4.0.3

### Patch Changes

- Updated dependencies [2cbf7b7]
- Updated dependencies [6b6ca69]
  - @pnpm/lockfile-file@9.0.1
  - @pnpm/license-scanner@3.0.1

## 4.0.2

### Patch Changes

- Updated dependencies [a80b539]
  - @pnpm/cli-utils@3.0.2

## 4.0.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0
  - @pnpm/cli-utils@3.0.1

## 4.0.0

### Major Changes

- f5766d9: `pnpm licenses list` prints license information of all versions of the same package in case different versions use different licenses. The format of the `pnpm licenses list --json` output has been changed [#7528](https://github.com/pnpm/pnpm/pull/7528).
- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- 3878ced: Fix `pnpm licenses list --json` command not returning correct paths when run on workspace members
- Updated dependencies [7733f3a]
- Updated dependencies [f5766d9]
- Updated dependencies [3ded840]
- Updated dependencies [c692f80]
- Updated dependencies [43cdd87]
- Updated dependencies [2d9e3b8]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [3477ee5]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [f67ad31]
- Updated dependencies [2b89155]
- Updated dependencies [60839fc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
  - @pnpm/config@21.0.0
  - @pnpm/license-scanner@3.0.0
  - @pnpm/error@6.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/common-cli-options-help@2.0.0
  - @pnpm/lockfile-file@9.0.0
  - @pnpm/store-path@9.0.0
  - @pnpm/cli-utils@3.0.0
  - @pnpm/command@5.0.0

## 3.0.14

### Patch Changes

- Updated dependencies [fd42caf24]
  - @pnpm/license-scanner@2.3.0
  - @pnpm/cli-utils@2.1.9
  - @pnpm/config@20.4.2

## 3.0.13

### Patch Changes

- @pnpm/license-scanner@2.2.10

## 3.0.12

### Patch Changes

- dcf3ef7e4: Handle Git repository names containing capital letters [#7488](https://github.com/pnpm/pnpm/pull/7488).
- Updated dependencies [37ccff637]
- Updated dependencies [d9564e354]
- Updated dependencies [fe737aeb4]
- Updated dependencies [dcf3ef7e4]
  - @pnpm/store-path@8.0.2
  - @pnpm/config@20.4.1
  - @pnpm/license-scanner@2.2.9
  - @pnpm/cli-utils@2.1.8

## 3.0.11

### Patch Changes

- Updated dependencies [c597f72ec]
  - @pnpm/config@20.4.0
  - @pnpm/cli-utils@2.1.7

## 3.0.10

### Patch Changes

- Updated dependencies [4e71066dd]
  - @pnpm/common-cli-options-help@1.1.0
  - @pnpm/config@20.3.0
  - @pnpm/cli-utils@2.1.6
  - @pnpm/license-scanner@2.2.8
  - @pnpm/lockfile-file@8.1.6

## 3.0.9

### Patch Changes

- Updated dependencies [672c559e4]
  - @pnpm/config@20.2.0
  - @pnpm/lockfile-file@8.1.5
  - @pnpm/license-scanner@2.2.7
  - @pnpm/cli-utils@2.1.5

## 3.0.8

### Patch Changes

- @pnpm/license-scanner@2.2.6

## 3.0.7

### Patch Changes

- @pnpm/cli-utils@2.1.4

## 3.0.6

### Patch Changes

- @pnpm/cli-utils@2.1.3

## 3.0.5

### Patch Changes

- @pnpm/license-scanner@2.2.5

## 3.0.4

### Patch Changes

- @pnpm/config@20.1.2
- @pnpm/license-scanner@2.2.4
- @pnpm/cli-utils@2.1.2

## 3.0.3

### Patch Changes

- @pnpm/license-scanner@2.2.3

## 3.0.2

### Patch Changes

- Updated dependencies [7d65d901a]
  - @pnpm/store-path@8.0.1
  - @pnpm/license-scanner@2.2.2
  - @pnpm/config@20.1.1
  - @pnpm/cli-utils@2.1.1

## 3.0.1

### Patch Changes

- @pnpm/license-scanner@2.2.1

## 3.0.0

### Major Changes

- d6592964f: `rootProjectManifestDir` is a required field.

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
- Updated dependencies [d6592964f]
  - @pnpm/license-scanner@2.2.0
  - @pnpm/cli-utils@2.1.0
  - @pnpm/config@20.1.0
  - @pnpm/lockfile-file@8.1.4

## 2.1.0

### Minor Changes

- fff7866f3: The `pnpm licenses list` command now accepts the `--filter` option to check the licenses of the dependencies of a subset of workspace projects [#5806](https://github.com/pnpm/pnpm/issues/5806).

### Patch Changes

- Updated dependencies [fff7866f3]
  - @pnpm/license-scanner@2.1.0

## 2.0.30

### Patch Changes

- Updated dependencies [ac5abd3ff]
- Updated dependencies [b60bb6cbe]
  - @pnpm/config@20.0.0
  - @pnpm/license-scanner@2.0.22
  - @pnpm/cli-utils@2.0.24

## 2.0.29

### Patch Changes

- Updated dependencies [b1dd0ee58]
  - @pnpm/config@19.2.1
  - @pnpm/cli-utils@2.0.23

## 2.0.28

### Patch Changes

- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/config@19.2.0
  - @pnpm/cli-utils@2.0.22
  - @pnpm/lockfile-file@8.1.3
  - @pnpm/license-scanner@2.0.21

## 2.0.27

### Patch Changes

- Updated dependencies [ee328fd25]
  - @pnpm/config@19.1.0
  - @pnpm/cli-utils@2.0.21
  - @pnpm/license-scanner@2.0.20

## 2.0.26

### Patch Changes

- @pnpm/cli-utils@2.0.20

## 2.0.25

### Patch Changes

- @pnpm/config@19.0.3
- @pnpm/license-scanner@2.0.19
- @pnpm/cli-utils@2.0.19

## 2.0.24

### Patch Changes

- @pnpm/config@19.0.2
- @pnpm/license-scanner@2.0.18
- @pnpm/cli-utils@2.0.18

## 2.0.23

### Patch Changes

- @pnpm/license-scanner@2.0.17
- @pnpm/config@19.0.1

## 2.0.22

### Patch Changes

- @pnpm/license-scanner@2.0.16
- @pnpm/config@19.0.1
- @pnpm/cli-utils@2.0.17

## 2.0.21

### Patch Changes

- Updated dependencies [cb8bcc8df]
  - @pnpm/config@19.0.0
  - @pnpm/license-scanner@2.0.15
  - @pnpm/cli-utils@2.0.16

## 2.0.20

### Patch Changes

- @pnpm/cli-utils@2.0.15

## 2.0.19

### Patch Changes

- @pnpm/cli-utils@2.0.14

## 2.0.18

### Patch Changes

- @pnpm/license-scanner@2.0.14
- @pnpm/config@18.4.4

## 2.0.17

### Patch Changes

- @pnpm/license-scanner@2.0.13
- @pnpm/config@18.4.4

## 2.0.16

### Patch Changes

- @pnpm/license-scanner@2.0.12
- @pnpm/config@18.4.4

## 2.0.15

### Patch Changes

- @pnpm/cli-utils@2.0.13
- @pnpm/config@18.4.4
- @pnpm/lockfile-file@8.1.2
- @pnpm/license-scanner@2.0.11

## 2.0.14

### Patch Changes

- @pnpm/cli-utils@2.0.12
- @pnpm/config@18.4.3
- @pnpm/license-scanner@2.0.10

## 2.0.13

### Patch Changes

- @pnpm/license-scanner@2.0.9
- @pnpm/config@18.4.2

## 2.0.12

### Patch Changes

- c686768f0: `pnpm license ls` should work even when there is a patched git protocol dependency [#6595](https://github.com/pnpm/pnpm/issues/6595)
- Updated dependencies [e2d631217]
- Updated dependencies [c686768f0]
  - @pnpm/config@18.4.2
  - @pnpm/license-scanner@2.0.8
  - @pnpm/cli-utils@2.0.11

## 2.0.11

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1
  - @pnpm/license-scanner@2.0.7
  - @pnpm/config@18.4.1
  - @pnpm/lockfile-file@8.1.1
  - @pnpm/error@5.0.2
  - @pnpm/cli-utils@2.0.10

## 2.0.10

### Patch Changes

- Updated dependencies [4b97f1f07]
  - @pnpm/license-scanner@2.0.6
  - @pnpm/config@18.4.0

## 2.0.9

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [301b8e2da]
  - @pnpm/lockfile-file@8.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/config@18.4.0
  - @pnpm/license-scanner@2.0.5
  - @pnpm/cli-utils@2.0.9
  - @pnpm/error@5.0.1

## 2.0.8

### Patch Changes

- Updated dependencies [ee429b300]
- Updated dependencies [1de07a4af]
  - @pnpm/cli-utils@2.0.8
  - @pnpm/config@18.3.2
  - @pnpm/license-scanner@2.0.4

## 2.0.7

### Patch Changes

- Updated dependencies [2809e89ab]
  - @pnpm/config@18.3.1
  - @pnpm/cli-utils@2.0.7

## 2.0.6

### Patch Changes

- Updated dependencies [32f8e08c6]
- Updated dependencies [c0760128d]
  - @pnpm/config@18.3.0
  - @pnpm/lockfile-file@8.0.2
  - @pnpm/cli-utils@2.0.6
  - @pnpm/license-scanner@2.0.3

## 2.0.5

### Patch Changes

- Updated dependencies [fc8780ca9]
  - @pnpm/config@18.2.0
  - @pnpm/license-scanner@2.0.2
  - @pnpm/cli-utils@2.0.5

## 2.0.4

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/lockfile-file@8.0.1
  - @pnpm/license-scanner@2.0.1
  - @pnpm/cli-utils@2.0.4
  - @pnpm/config@18.1.1

## 2.0.3

### Patch Changes

- Updated dependencies [e2cb4b63d]
- Updated dependencies [cd6ce11f0]
  - @pnpm/config@18.1.0
  - @pnpm/cli-utils@2.0.3

## 2.0.2

### Patch Changes

- @pnpm/config@18.0.2
- @pnpm/cli-utils@2.0.2

## 2.0.1

### Patch Changes

- @pnpm/config@18.0.1
- @pnpm/cli-utils@2.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- 22ccf155e: Fix `Segmentation fault` error in the bundled version of pnpm [#6241](https://github.com/pnpm/pnpm/issues/6241).
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [158d8cf22]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [8e35c21d1]
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [113f0ae26]
- Updated dependencies [417c8ac59]
  - @pnpm/config@18.0.0
  - @pnpm/lockfile-file@8.0.0
  - @pnpm/common-cli-options-help@1.0.0
  - @pnpm/license-scanner@2.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/store-path@8.0.0
  - @pnpm/error@5.0.0
  - @pnpm/cli-utils@2.0.0
  - @pnpm/command@4.0.0

## 1.0.33

### Patch Changes

- @pnpm/config@17.0.2
- @pnpm/cli-utils@1.1.7

## 1.0.32

### Patch Changes

- Updated dependencies [b38d711f3]
  - @pnpm/config@17.0.1
  - @pnpm/cli-utils@1.1.6

## 1.0.31

### Patch Changes

- Updated dependencies [787c43dcc]
- Updated dependencies [e505b58e3]
  - @pnpm/lockfile-file@7.0.6
  - @pnpm/config@17.0.0
  - @pnpm/license-scanner@1.0.17
  - @pnpm/cli-utils@1.1.5

## 1.0.30

### Patch Changes

- @pnpm/config@16.7.2
- @pnpm/cli-utils@1.1.4

## 1.0.29

### Patch Changes

- 019e4f2de: Should not throw an error when local dependency use file protocol [#6115](https://github.com/pnpm/pnpm/issues/6115).
- Updated dependencies [019e4f2de]
  - @pnpm/license-scanner@1.0.16
  - @pnpm/config@16.7.1
  - @pnpm/cli-utils@1.1.3

## 1.0.28

### Patch Changes

- Updated dependencies [7d64d757b]
- Updated dependencies [5c31fa8be]
  - @pnpm/cli-utils@1.1.2
  - @pnpm/config@16.7.0

## 1.0.27

### Patch Changes

- Updated dependencies [ed946c73e]
  - @pnpm/lockfile-file@7.0.5
  - @pnpm/license-scanner@1.0.15
  - @pnpm/config@16.6.4
  - @pnpm/cli-utils@1.1.1

## 1.0.26

### Patch Changes

- Updated dependencies [0377d9367]
  - @pnpm/cli-utils@1.1.0
  - @pnpm/config@16.6.3

## 1.0.25

### Patch Changes

- @pnpm/config@16.6.2
- @pnpm/cli-utils@1.0.34

## 1.0.24

### Patch Changes

- @pnpm/config@16.6.1
- @pnpm/license-scanner@1.0.14
- @pnpm/cli-utils@1.0.33

## 1.0.23

### Patch Changes

- Updated dependencies [59ee53678]
  - @pnpm/config@16.6.0
  - @pnpm/lockfile-file@7.0.4
  - @pnpm/license-scanner@1.0.13
  - @pnpm/cli-utils@1.0.32

## 1.0.22

### Patch Changes

- @pnpm/lockfile-file@7.0.3
- @pnpm/license-scanner@1.0.12
- @pnpm/config@16.5.5
- @pnpm/cli-utils@1.0.31

## 1.0.21

### Patch Changes

- @pnpm/config@16.5.4
- @pnpm/cli-utils@1.0.30

## 1.0.20

### Patch Changes

- @pnpm/config@16.5.3
- @pnpm/cli-utils@1.0.29

## 1.0.19

### Patch Changes

- @pnpm/config@16.5.2
- @pnpm/cli-utils@1.0.28

## 1.0.18

### Patch Changes

- @pnpm/license-scanner@1.0.11
- @pnpm/config@16.5.1
- @pnpm/cli-utils@1.0.27

## 1.0.17

### Patch Changes

- Updated dependencies [28b47a156]
  - @pnpm/config@16.5.0
  - @pnpm/cli-utils@1.0.26

## 1.0.16

### Patch Changes

- @pnpm/license-scanner@1.0.10
- @pnpm/config@16.4.3
- @pnpm/cli-utils@1.0.25

## 1.0.15

### Patch Changes

- Updated dependencies [9a68ebbae]
  - @pnpm/lockfile-file@7.0.2
  - @pnpm/license-scanner@1.0.9
  - @pnpm/config@16.4.2
  - @pnpm/cli-utils@1.0.24

## 1.0.14

### Patch Changes

- @pnpm/lockfile-file@7.0.1
- @pnpm/license-scanner@1.0.8
- @pnpm/config@16.4.1
- @pnpm/cli-utils@1.0.23

## 1.0.13

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/lockfile-file@7.0.0
  - @pnpm/config@16.4.0
  - @pnpm/error@4.0.1
  - @pnpm/license-scanner@1.0.7
  - @pnpm/cli-utils@1.0.22

## 1.0.12

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/config@16.3.0
  - @pnpm/cli-utils@1.0.21

## 1.0.11

### Patch Changes

- @pnpm/cli-utils@1.0.20
- @pnpm/config@16.2.2

## 1.0.10

### Patch Changes

- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1
  - @pnpm/cli-utils@1.0.19

## 1.0.9

### Patch Changes

- 1d3995fe3: Add the 'description'-field to the licenses output [#5836](https://github.com/pnpm/pnpm/pull/5836).
- Updated dependencies [1d3995fe3]
- Updated dependencies [841f52e70]
  - @pnpm/license-scanner@1.0.6
  - @pnpm/config@16.2.0
  - @pnpm/cli-utils@1.0.18

## 1.0.8

### Patch Changes

- @pnpm/cli-utils@1.0.17
- @pnpm/config@16.1.11
- @pnpm/lockfile-file@6.0.5
- @pnpm/license-scanner@1.0.5

## 1.0.7

### Patch Changes

- @pnpm/lockfile-file@6.0.4
- @pnpm/license-scanner@1.0.4
- @pnpm/config@16.1.10
- @pnpm/cli-utils@1.0.16

## 1.0.6

### Patch Changes

- 5464e1da6: `pnpm license list` should not fail if a license file is an executable [#5740](https://github.com/pnpm/pnpm/pull/5740).
- Updated dependencies [5464e1da6]
  - @pnpm/license-scanner@1.0.3
  - @pnpm/config@16.1.9
  - @pnpm/cli-utils@1.0.15

## 1.0.5

### Patch Changes

- 568dc3ab2: `pnpm licenses` should print help, not just an error message [#5745](https://github.com/pnpm/pnpm/issues/5745).
  - @pnpm/cli-utils@1.0.14
  - @pnpm/config@16.1.8

## 1.0.4

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/config@16.1.7
  - @pnpm/lockfile-file@6.0.3
  - @pnpm/cli-utils@1.0.13
  - @pnpm/license-scanner@1.0.2

## 1.0.3

### Patch Changes

- @pnpm/config@16.1.6
- @pnpm/cli-utils@1.0.12

## 1.0.2

### Patch Changes

- @pnpm/config@16.1.5
- @pnpm/cli-utils@1.0.11

## 1.0.1

### Patch Changes

- a8cc22364: Fix the CLI help of the `pnpm licenses` command.
  - @pnpm/cli-utils@1.0.10
  - @pnpm/config@16.1.4
  - @pnpm/license-scanner@1.0.1

## 1.0.0

### Major Changes

- d84a30a04: Added a new command `pnpm licenses list`, which displays the licenses of the packages [#2825](https://github.com/pnpm/pnpm/issues/2825)

### Patch Changes

- Updated dependencies [d84a30a04]
  - @pnpm/license-scanner@1.0.0
  - @pnpm/config@16.1.3
  - @pnpm/cli-utils@1.0.9
