# @pnpm/cli-utils

## 3.1.3

### Patch Changes

- Updated dependencies [1b03682]
- Updated dependencies [9bf9f71]
- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/config@21.6.0
  - @pnpm/default-reporter@13.1.6
  - @pnpm/types@11.0.0
  - @pnpm/cli-meta@6.0.3
  - @pnpm/package-is-installable@9.0.4
  - @pnpm/manifest-utils@6.0.4
  - @pnpm/read-project-manifest@6.0.4

## 3.1.2

### Patch Changes

- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [13e55b2]
- Updated dependencies [04b8363]
  - @pnpm/config@21.5.0
  - @pnpm/types@10.1.1
  - @pnpm/default-reporter@13.1.5
  - @pnpm/cli-meta@6.0.2
  - @pnpm/package-is-installable@9.0.3
  - @pnpm/manifest-utils@6.0.3
  - @pnpm/read-project-manifest@6.0.3

## 3.1.1

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/config@21.4.0
  - @pnpm/default-reporter@13.1.4

## 3.1.0

### Minor Changes

- b7ca13f: If `package-manager-strict-version` is set to `true` pnpm will fail if its version will not exactly match the version in the `packageManager` field of `package.json`.

### Patch Changes

- b7ca13f: pnpm doesn't fail if its version doesn't match the one specified in the "packageManager" field of `package.json` [#8087](https://github.com/pnpm/pnpm/issues/8087).
- Updated dependencies [b7ca13f]
  - @pnpm/config@21.3.0
  - @pnpm/default-reporter@13.1.3

## 3.0.7

### Patch Changes

- @pnpm/config@21.2.3
- @pnpm/default-reporter@13.1.2

## 3.0.6

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/cli-meta@6.0.1
  - @pnpm/default-reporter@13.1.1
  - @pnpm/config@21.2.2
  - @pnpm/package-is-installable@9.0.2
  - @pnpm/manifest-utils@6.0.2
  - @pnpm/read-project-manifest@6.0.2

## 3.0.5

### Patch Changes

- Updated dependencies [a7aef51]
- Updated dependencies [524990f]
  - @pnpm/error@6.0.1
  - @pnpm/default-reporter@13.1.0
  - @pnpm/config@21.2.1
  - @pnpm/package-is-installable@9.0.1
  - @pnpm/manifest-utils@6.0.1
  - @pnpm/read-project-manifest@6.0.1

## 3.0.4

### Patch Changes

- Updated dependencies [43b6bb7]
  - @pnpm/default-reporter@13.0.3

## 3.0.3

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/config@21.2.0
  - @pnpm/default-reporter@13.0.2

## 3.0.2

### Patch Changes

- a80b539: Print a hint about the `package-manager-strict` setting, when pnpm doesn't match the version specified in the `packageManager` field in `package.json`.

## 3.0.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0
  - @pnpm/default-reporter@13.0.1

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- 3477ee5: pnpm will now check the `package.json` file for a `packageManager` field. If this field is present and specifies a different package manager or a different version of pnpm than the one you're currently using, pnpm will not proceed. This ensures that you're always using the correct package manager and version that the project requires.

  To disable this behaviour, set the `package-manager-strict` setting to `false` or the `COREPACK_ENABLE_STRICT` env variable to `0`.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [aa33269]
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [2d9e3b8]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [2b89155]
- Updated dependencies [60839fc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
  - @pnpm/types@10.0.0
  - @pnpm/config@21.0.0
  - @pnpm/default-reporter@13.0.0
  - @pnpm/error@6.0.0
  - @pnpm/read-project-manifest@6.0.0
  - @pnpm/package-is-installable@9.0.0
  - @pnpm/manifest-utils@6.0.0
  - @pnpm/cli-meta@6.0.0

## 2.1.9

### Patch Changes

- Updated dependencies [f12884def]
  - @pnpm/default-reporter@12.4.13
  - @pnpm/config@20.4.2

## 2.1.8

### Patch Changes

- Updated dependencies [d9564e354]
  - @pnpm/config@20.4.1
  - @pnpm/default-reporter@12.4.12

## 2.1.7

### Patch Changes

- Updated dependencies [fac2ed424]
- Updated dependencies [c597f72ec]
  - @pnpm/default-reporter@12.4.11
  - @pnpm/config@20.4.0

## 2.1.6

### Patch Changes

- Updated dependencies [4e71066dd]
- Updated dependencies [4d34684f1]
  - @pnpm/config@20.3.0
  - @pnpm/types@9.4.2
  - @pnpm/default-reporter@12.4.10
  - @pnpm/cli-meta@5.0.6
  - @pnpm/package-is-installable@8.1.2
  - @pnpm/manifest-utils@5.0.7
  - @pnpm/read-project-manifest@5.0.10

## 2.1.5

### Patch Changes

- Updated dependencies
- Updated dependencies [672c559e4]
  - @pnpm/types@9.4.1
  - @pnpm/config@20.2.0
  - @pnpm/cli-meta@5.0.5
  - @pnpm/default-reporter@12.4.9
  - @pnpm/package-is-installable@8.1.1
  - @pnpm/manifest-utils@5.0.6
  - @pnpm/read-project-manifest@5.0.9

## 2.1.4

### Patch Changes

- Updated dependencies [633c0d6f8]
  - @pnpm/default-reporter@12.4.8

## 2.1.3

### Patch Changes

- Updated dependencies [45bdc79b1]
  - @pnpm/default-reporter@12.4.7

## 2.1.2

### Patch Changes

- @pnpm/config@20.1.2
- @pnpm/default-reporter@12.4.6

## 2.1.1

### Patch Changes

- @pnpm/config@20.1.1
- @pnpm/default-reporter@12.4.5

## 2.1.0

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
  - @pnpm/package-is-installable@8.1.0
  - @pnpm/types@9.4.0
  - @pnpm/config@20.1.0
  - @pnpm/cli-meta@5.0.4
  - @pnpm/default-reporter@12.4.4
  - @pnpm/manifest-utils@5.0.5
  - @pnpm/read-project-manifest@5.0.8

## 2.0.24

### Patch Changes

- Updated dependencies [ac5abd3ff]
- Updated dependencies [b60bb6cbe]
  - @pnpm/config@20.0.0
  - @pnpm/default-reporter@12.4.3

## 2.0.23

### Patch Changes

- Updated dependencies [b1dd0ee58]
  - @pnpm/config@19.2.1
  - @pnpm/default-reporter@12.4.2

## 2.0.22

### Patch Changes

- Updated dependencies [d774a3196]
- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/config@19.2.0
  - @pnpm/types@9.3.0
  - @pnpm/default-reporter@12.4.1
  - @pnpm/cli-meta@5.0.3
  - @pnpm/package-is-installable@8.0.5
  - @pnpm/manifest-utils@5.0.4
  - @pnpm/read-project-manifest@5.0.7

## 2.0.21

### Patch Changes

- Updated dependencies [ee328fd25]
  - @pnpm/default-reporter@12.4.0
  - @pnpm/config@19.1.0

## 2.0.20

### Patch Changes

- Updated dependencies [61b9ca189]
  - @pnpm/default-reporter@12.3.5

## 2.0.19

### Patch Changes

- @pnpm/read-project-manifest@5.0.6
- @pnpm/config@19.0.3
- @pnpm/default-reporter@12.3.4

## 2.0.18

### Patch Changes

- @pnpm/config@19.0.2
- @pnpm/default-reporter@12.3.3

## 2.0.17

### Patch Changes

- @pnpm/config@19.0.1
- @pnpm/default-reporter@12.3.2

## 2.0.16

### Patch Changes

- Updated dependencies [cb8bcc8df]
- Updated dependencies [cc785f7e1]
  - @pnpm/config@19.0.0
  - @pnpm/default-reporter@12.3.1
  - @pnpm/read-project-manifest@5.0.5

## 2.0.15

### Patch Changes

- Updated dependencies [8a4dac63c]
  - @pnpm/default-reporter@12.2.9

## 2.0.14

### Patch Changes

- Updated dependencies [25396e3c5]
- Updated dependencies [751c157cd]
  - @pnpm/default-reporter@12.2.8

## 2.0.13

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/cli-meta@5.0.2
  - @pnpm/default-reporter@12.2.7
  - @pnpm/config@18.4.4
  - @pnpm/package-is-installable@8.0.4
  - @pnpm/manifest-utils@5.0.3
  - @pnpm/read-project-manifest@5.0.4

## 2.0.12

### Patch Changes

- Updated dependencies [b4892acc5]
  - @pnpm/read-project-manifest@5.0.3
  - @pnpm/config@18.4.3
  - @pnpm/default-reporter@12.2.6

## 2.0.11

### Patch Changes

- Updated dependencies [100d03b36]
- Updated dependencies [e2d631217]
  - @pnpm/default-reporter@12.2.5
  - @pnpm/config@18.4.2

## 2.0.10

### Patch Changes

- @pnpm/config@18.4.1
- @pnpm/error@5.0.2
- @pnpm/default-reporter@12.2.4
- @pnpm/package-is-installable@8.0.3
- @pnpm/manifest-utils@5.0.2
- @pnpm/read-project-manifest@5.0.2

## 2.0.9

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [301b8e2da]
  - @pnpm/types@9.1.0
  - @pnpm/manifest-utils@5.0.1
  - @pnpm/config@18.4.0
  - @pnpm/cli-meta@5.0.1
  - @pnpm/default-reporter@12.2.3
  - @pnpm/package-is-installable@8.0.2
  - @pnpm/read-project-manifest@5.0.1
  - @pnpm/error@5.0.1

## 2.0.8

### Patch Changes

- ee429b300: Expanded missing command error, including 'did you mean' [#6492](https://github.com/pnpm/pnpm/issues/6492).
- Updated dependencies [1de07a4af]
  - @pnpm/config@18.3.2
  - @pnpm/default-reporter@12.2.2

## 2.0.7

### Patch Changes

- Updated dependencies [2809e89ab]
  - @pnpm/config@18.3.1
  - @pnpm/default-reporter@12.2.1

## 2.0.6

### Patch Changes

- Updated dependencies [32f8e08c6]
- Updated dependencies [31ca5a218]
- Updated dependencies [c0760128d]
- Updated dependencies [6850bb135]
  - @pnpm/config@18.3.0
  - @pnpm/default-reporter@12.2.0
  - @pnpm/package-is-installable@8.0.1

## 2.0.5

### Patch Changes

- Updated dependencies [6cfaf31a1]
- Updated dependencies [fc8780ca9]
  - @pnpm/default-reporter@12.1.0
  - @pnpm/config@18.2.0

## 2.0.4

### Patch Changes

- Updated dependencies [af3e5559d]
  - @pnpm/default-reporter@12.0.4
  - @pnpm/config@18.1.1

## 2.0.3

### Patch Changes

- Updated dependencies [e2cb4b63d]
- Updated dependencies [cd6ce11f0]
  - @pnpm/config@18.1.0
  - @pnpm/default-reporter@12.0.3

## 2.0.2

### Patch Changes

- @pnpm/config@18.0.2
- @pnpm/default-reporter@12.0.2

## 2.0.1

### Patch Changes

- @pnpm/config@18.0.1
- @pnpm/default-reporter@12.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [158d8cf22]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [8e35c21d1]
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [113f0ae26]
  - @pnpm/config@18.0.0
  - @pnpm/read-project-manifest@5.0.0
  - @pnpm/package-is-installable@8.0.0
  - @pnpm/manifest-utils@5.0.0
  - @pnpm/default-reporter@12.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0
  - @pnpm/cli-meta@5.0.0

## 1.1.7

### Patch Changes

- @pnpm/config@17.0.2
- @pnpm/default-reporter@11.0.42

## 1.1.6

### Patch Changes

- Updated dependencies [b38d711f3]
  - @pnpm/config@17.0.1
  - @pnpm/default-reporter@11.0.41

## 1.1.5

### Patch Changes

- Updated dependencies [e505b58e3]
  - @pnpm/config@17.0.0
  - @pnpm/read-project-manifest@4.1.4
  - @pnpm/default-reporter@11.0.40

## 1.1.4

### Patch Changes

- @pnpm/config@16.7.2
- @pnpm/default-reporter@11.0.39

## 1.1.3

### Patch Changes

- @pnpm/config@16.7.1
- @pnpm/default-reporter@11.0.38

## 1.1.2

### Patch Changes

- 7d64d757b: Add `skipped` status in exec report summary when script is missing [#6139](https://github.com/pnpm/pnpm/pull/6139).
- Updated dependencies [5c31fa8be]
  - @pnpm/config@16.7.0
  - @pnpm/default-reporter@11.0.37

## 1.1.1

### Patch Changes

- @pnpm/config@16.6.4
- @pnpm/default-reporter@11.0.36

## 1.1.0

### Minor Changes

- 0377d9367: Add --report-summary for pnpm exec and pnpm run [#6008](https://github.com/pnpm/pnpm/issues/6008)

### Patch Changes

- @pnpm/config@16.6.3
- @pnpm/default-reporter@11.0.35

## 1.0.34

### Patch Changes

- @pnpm/config@16.6.2
- @pnpm/default-reporter@11.0.34

## 1.0.33

### Patch Changes

- @pnpm/config@16.6.1
- @pnpm/default-reporter@11.0.33

## 1.0.32

### Patch Changes

- Updated dependencies [59ee53678]
  - @pnpm/config@16.6.0
  - @pnpm/default-reporter@11.0.32

## 1.0.31

### Patch Changes

- @pnpm/config@16.5.5
- @pnpm/default-reporter@11.0.31

## 1.0.30

### Patch Changes

- @pnpm/config@16.5.4
- @pnpm/default-reporter@11.0.30

## 1.0.29

### Patch Changes

- @pnpm/config@16.5.3
- @pnpm/default-reporter@11.0.29

## 1.0.28

### Patch Changes

- @pnpm/config@16.5.2
- @pnpm/default-reporter@11.0.28

## 1.0.27

### Patch Changes

- @pnpm/config@16.5.1
- @pnpm/default-reporter@11.0.27

## 1.0.26

### Patch Changes

- Updated dependencies [28b47a156]
  - @pnpm/config@16.5.0
  - @pnpm/default-reporter@11.0.26

## 1.0.25

### Patch Changes

- @pnpm/config@16.4.3
- @pnpm/default-reporter@11.0.25

## 1.0.24

### Patch Changes

- @pnpm/config@16.4.2
- @pnpm/default-reporter@11.0.24

## 1.0.23

### Patch Changes

- @pnpm/config@16.4.1
- @pnpm/default-reporter@11.0.23

## 1.0.22

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/config@16.4.0
  - @pnpm/error@4.0.1
  - @pnpm/default-reporter@11.0.22
  - @pnpm/package-is-installable@7.0.4
  - @pnpm/manifest-utils@4.1.4
  - @pnpm/read-project-manifest@4.1.3

## 1.0.21

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/config@16.3.0
  - @pnpm/default-reporter@11.0.21

## 1.0.20

### Patch Changes

- Updated dependencies [ec97a3105]
  - @pnpm/default-reporter@11.0.20
  - @pnpm/config@16.2.2

## 1.0.19

### Patch Changes

- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1
  - @pnpm/default-reporter@11.0.19

## 1.0.18

### Patch Changes

- Updated dependencies [0048e0e64]
- Updated dependencies [841f52e70]
  - @pnpm/default-reporter@11.0.18
  - @pnpm/config@16.2.0

## 1.0.17

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/cli-meta@4.0.3
  - @pnpm/default-reporter@11.0.17
  - @pnpm/config@16.1.11
  - @pnpm/package-is-installable@7.0.3
  - @pnpm/manifest-utils@4.1.3
  - @pnpm/read-project-manifest@4.1.2

## 1.0.16

### Patch Changes

- @pnpm/config@16.1.10
- @pnpm/default-reporter@11.0.16

## 1.0.15

### Patch Changes

- @pnpm/config@16.1.9
- @pnpm/default-reporter@11.0.15

## 1.0.14

### Patch Changes

- Updated dependencies [3f644a514]
  - @pnpm/default-reporter@11.0.14
  - @pnpm/config@16.1.8

## 1.0.13

### Patch Changes

- Updated dependencies [c245edf1b]
- Updated dependencies [a9d59d8bc]
  - @pnpm/manifest-utils@4.1.2
  - @pnpm/config@16.1.7
  - @pnpm/default-reporter@11.0.13
  - @pnpm/read-project-manifest@4.1.1

## 1.0.12

### Patch Changes

- @pnpm/config@16.1.6
- @pnpm/default-reporter@11.0.12

## 1.0.11

### Patch Changes

- @pnpm/config@16.1.5
- @pnpm/default-reporter@11.0.11

## 1.0.10

### Patch Changes

- Updated dependencies [fec9e3149]
- Updated dependencies [0d12d38fd]
  - @pnpm/read-project-manifest@4.1.0
  - @pnpm/config@16.1.4
  - @pnpm/default-reporter@11.0.10

## 1.0.9

### Patch Changes

- @pnpm/config@16.1.3
- @pnpm/default-reporter@11.0.9

## 1.0.8

### Patch Changes

- @pnpm/config@16.1.2
- @pnpm/default-reporter@11.0.8

## 1.0.7

### Patch Changes

- @pnpm/config@16.1.1
- @pnpm/default-reporter@11.0.7

## 1.0.6

### Patch Changes

- Updated dependencies [3dab7f83c]
  - @pnpm/config@16.1.0
  - @pnpm/default-reporter@11.0.6

## 1.0.5

### Patch Changes

- Updated dependencies [a4c58d424]
- Updated dependencies [702e847c1]
  - @pnpm/default-reporter@11.0.5
  - @pnpm/types@8.9.0
  - @pnpm/cli-meta@4.0.2
  - @pnpm/config@16.0.5
  - @pnpm/manifest-utils@4.1.1
  - @pnpm/package-is-installable@7.0.2
  - @pnpm/read-project-manifest@4.0.2

## 1.0.4

### Patch Changes

- @pnpm/config@16.0.4
- @pnpm/default-reporter@11.0.4

## 1.0.3

### Patch Changes

- Updated dependencies [0018cd03e]
- Updated dependencies [aacb83f73]
- Updated dependencies [a14ad09e6]
  - @pnpm/default-reporter@11.0.3
  - @pnpm/config@16.0.3

## 1.0.2

### Patch Changes

- Updated dependencies [bea0acdfc]
  - @pnpm/config@16.0.2
  - @pnpm/default-reporter@11.0.2

## 1.0.1

### Patch Changes

- Updated dependencies [e7fd8a84c]
- Updated dependencies [844e82f3a]
- Updated dependencies [844e82f3a]
  - @pnpm/config@16.0.1
  - @pnpm/types@8.8.0
  - @pnpm/manifest-utils@4.1.0
  - @pnpm/default-reporter@11.0.1
  - @pnpm/cli-meta@4.0.1
  - @pnpm/package-is-installable@7.0.1
  - @pnpm/read-project-manifest@4.0.1

## 1.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [1d0fd82fd]
- Updated dependencies [645384bfd]
- Updated dependencies [f884689e0]
- Updated dependencies [3c117996e]
  - @pnpm/cli-meta@4.0.0
  - @pnpm/config@16.0.0
  - @pnpm/default-reporter@11.0.0
  - @pnpm/error@4.0.0
  - @pnpm/manifest-utils@4.0.0
  - @pnpm/package-is-installable@7.0.0
  - @pnpm/read-project-manifest@4.0.0

## 0.7.43

### Patch Changes

- @pnpm/read-project-manifest@3.0.13
- @pnpm/config@15.10.12
- @pnpm/default-reporter@10.1.1

## 0.7.42

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/default-reporter@10.1.0
  - @pnpm/manifest-utils@3.1.6
  - @pnpm/package-is-installable@6.0.12
  - @pnpm/config@15.10.11

## 0.7.41

### Patch Changes

- Updated dependencies [e8a631bf0]
- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/default-reporter@10.0.1
  - @pnpm/config@15.10.10
  - @pnpm/manifest-utils@3.1.5
  - @pnpm/package-is-installable@6.0.11
  - @pnpm/read-project-manifest@3.0.12

## 0.7.40

### Patch Changes

- Updated dependencies [51566e34b]
- Updated dependencies [d665f3ff7]
  - @pnpm/default-reporter@10.0.0
  - @pnpm/types@8.7.0
  - @pnpm/config@15.10.9
  - @pnpm/cli-meta@3.0.8
  - @pnpm/manifest-utils@3.1.4
  - @pnpm/package-is-installable@6.0.10
  - @pnpm/read-project-manifest@3.0.11

## 0.7.39

### Patch Changes

- @pnpm/config@15.10.8
- @pnpm/default-reporter@9.1.28

## 0.7.38

### Patch Changes

- @pnpm/config@15.10.7
- @pnpm/default-reporter@9.1.27

## 0.7.37

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/cli-meta@3.0.7
  - @pnpm/config@15.10.6
  - @pnpm/default-reporter@9.1.26
  - @pnpm/manifest-utils@3.1.3
  - @pnpm/package-is-installable@6.0.9
  - @pnpm/read-project-manifest@3.0.10

## 0.7.36

### Patch Changes

- @pnpm/config@15.10.5
- @pnpm/default-reporter@9.1.25

## 0.7.35

### Patch Changes

- Updated dependencies [728c0cdf6]
  - @pnpm/default-reporter@9.1.24
  - @pnpm/config@15.10.4

## 0.7.34

### Patch Changes

- @pnpm/config@15.10.3
- @pnpm/default-reporter@9.1.23

## 0.7.33

### Patch Changes

- @pnpm/config@15.10.2
- @pnpm/default-reporter@9.1.22

## 0.7.32

### Patch Changes

- @pnpm/config@15.10.1
- @pnpm/default-reporter@9.1.21

## 0.7.31

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/config@15.10.0
  - @pnpm/default-reporter@9.1.20

## 0.7.30

### Patch Changes

- @pnpm/config@15.9.4
- @pnpm/default-reporter@9.1.19

## 0.7.29

### Patch Changes

- @pnpm/config@15.9.3
- @pnpm/default-reporter@9.1.18

## 0.7.28

### Patch Changes

- @pnpm/config@15.9.2
- @pnpm/default-reporter@9.1.17

## 0.7.27

### Patch Changes

- @pnpm/config@15.9.1
- @pnpm/default-reporter@9.1.16

## 0.7.26

### Patch Changes

- Updated dependencies [39c040127]
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
  - @pnpm/default-reporter@9.1.15
  - @pnpm/read-project-manifest@3.0.9
  - @pnpm/config@15.9.0

## 0.7.25

### Patch Changes

- Updated dependencies [c90798461]
- Updated dependencies [34121d753]
  - @pnpm/types@8.5.0
  - @pnpm/config@15.8.1
  - @pnpm/cli-meta@3.0.6
  - @pnpm/default-reporter@9.1.14
  - @pnpm/manifest-utils@3.1.2
  - @pnpm/package-is-installable@6.0.8
  - @pnpm/read-project-manifest@3.0.8

## 0.7.24

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0
  - @pnpm/default-reporter@9.1.13

## 0.7.23

### Patch Changes

- @pnpm/config@15.7.1
- @pnpm/default-reporter@9.1.12

## 0.7.22

### Patch Changes

- Updated dependencies [01c5834bf]
- Updated dependencies [4fa1091c8]
  - @pnpm/read-project-manifest@3.0.7
  - @pnpm/config@15.7.0
  - @pnpm/default-reporter@9.1.11

## 0.7.21

### Patch Changes

- Updated dependencies [7334b347b]
- Updated dependencies [e3f4d131c]
  - @pnpm/config@15.6.1
  - @pnpm/manifest-utils@3.1.1
  - @pnpm/default-reporter@9.1.10

## 0.7.20

### Patch Changes

- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/config@15.6.0
  - @pnpm/default-reporter@9.1.9

## 0.7.19

### Patch Changes

- Updated dependencies [c71215041]
  - @pnpm/default-reporter@9.1.8
  - @pnpm/config@15.5.2

## 0.7.18

### Patch Changes

- Updated dependencies [f5621a42c]
  - @pnpm/manifest-utils@3.1.0

## 0.7.17

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/config@15.5.1
  - @pnpm/default-reporter@9.1.7

## 0.7.16

### Patch Changes

- Updated dependencies [f48d46ef6]
  - @pnpm/config@15.5.0
  - @pnpm/default-reporter@9.1.6

## 0.7.15

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/cli-meta@3.0.5
  - @pnpm/config@15.4.1
  - @pnpm/default-reporter@9.1.5
  - @pnpm/manifest-utils@3.0.6
  - @pnpm/package-is-installable@6.0.7
  - @pnpm/read-project-manifest@3.0.6

## 0.7.14

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/types@8.3.0
  - @pnpm/config@15.4.0
  - @pnpm/cli-meta@3.0.4
  - @pnpm/default-reporter@9.1.4
  - @pnpm/manifest-utils@3.0.5
  - @pnpm/package-is-installable@6.0.6
  - @pnpm/read-project-manifest@3.0.5

## 0.7.13

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
  - @pnpm/types@8.2.0
  - @pnpm/config@15.3.0
  - @pnpm/cli-meta@3.0.3
  - @pnpm/default-reporter@9.1.3
  - @pnpm/manifest-utils@3.0.4
  - @pnpm/package-is-installable@6.0.5
  - @pnpm/read-project-manifest@3.0.4

## 0.7.12

### Patch Changes

- Updated dependencies [25798aad1]
  - @pnpm/config@15.2.1
  - @pnpm/default-reporter@9.1.2

## 0.7.11

### Patch Changes

- Updated dependencies [4d39e4a0c]
- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
- Updated dependencies [9b7941c81]
  - @pnpm/types@8.1.0
  - @pnpm/config@15.2.0
  - @pnpm/default-reporter@9.1.1
  - @pnpm/cli-meta@3.0.2
  - @pnpm/manifest-utils@3.0.3
  - @pnpm/package-is-installable@6.0.4
  - @pnpm/read-project-manifest@3.0.3

## 0.7.10

### Patch Changes

- Updated dependencies [2493b8ef3]
  - @pnpm/default-reporter@9.1.0
  - @pnpm/config@15.1.4

## 0.7.9

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4
  - @pnpm/default-reporter@9.0.8

## 0.7.8

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3
  - @pnpm/default-reporter@9.0.7

## 0.7.7

### Patch Changes

- Updated dependencies [190f0b331]
  - @pnpm/default-reporter@9.0.6

## 0.7.6

### Patch Changes

- Updated dependencies [af22c6c4f]
  - @pnpm/config@15.1.2
  - @pnpm/default-reporter@9.0.5

## 0.7.5

### Patch Changes

- 52b0576af: feat: support libc filed
- Updated dependencies [52b0576af]
  - @pnpm/package-is-installable@6.0.3

## 0.7.4

### Patch Changes

- Updated dependencies [3b98e43a9]
  - @pnpm/default-reporter@9.0.4
  - @pnpm/config@15.1.1

## 0.7.3

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/cli-meta@3.0.1
  - @pnpm/config@15.1.1
  - @pnpm/default-reporter@9.0.3
  - @pnpm/manifest-utils@3.0.2
  - @pnpm/package-is-installable@6.0.2
  - @pnpm/read-project-manifest@3.0.2

## 0.7.2

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0
  - @pnpm/default-reporter@9.0.2

## 0.7.1

### Patch Changes

- Updated dependencies [8dac029ef]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
- Updated dependencies [e94149987]
- Updated dependencies [618842b0d]
  - @pnpm/config@15.0.0
  - @pnpm/default-reporter@9.0.1
  - @pnpm/manifest-utils@3.0.1
  - @pnpm/error@3.0.1
  - @pnpm/package-is-installable@6.0.1
  - @pnpm/read-project-manifest@3.0.1

## 0.7.0

### Minor Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/types@8.0.0
  - @pnpm/cli-meta@3.0.0
  - @pnpm/default-reporter@9.0.0
  - @pnpm/error@3.0.0
  - @pnpm/manifest-utils@3.0.0
  - @pnpm/package-is-installable@6.0.0
  - @pnpm/read-project-manifest@3.0.0

## 0.6.50

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/config@13.13.2
  - @pnpm/default-reporter@8.5.13
  - @pnpm/manifest-utils@2.1.9
  - @pnpm/package-is-installable@5.0.13
  - @pnpm/read-project-manifest@2.0.13

## 0.6.49

### Patch Changes

- Updated dependencies [b138d048c]
- Updated dependencies [5f00eb0e0]
  - @pnpm/types@7.10.0
  - @pnpm/default-reporter@8.5.12
  - @pnpm/cli-meta@2.0.2
  - @pnpm/config@13.13.1
  - @pnpm/manifest-utils@2.1.8
  - @pnpm/package-is-installable@5.0.12
  - @pnpm/read-project-manifest@2.0.12

## 0.6.48

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0
  - @pnpm/default-reporter@8.5.11

## 0.6.47

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0
  - @pnpm/default-reporter@8.5.10

## 0.6.46

### Patch Changes

- Updated dependencies [fff0e4493]
- Updated dependencies [8a2cad034]
  - @pnpm/config@13.11.0
  - @pnpm/manifest-utils@2.1.7
  - @pnpm/default-reporter@8.5.9

## 0.6.45

### Patch Changes

- Updated dependencies [a1ffef5ca]
  - @pnpm/default-reporter@8.5.8

## 0.6.44

### Patch Changes

- Updated dependencies [e76151f66]
- Updated dependencies [26cd01b88]
  - @pnpm/config@13.10.0
  - @pnpm/types@7.9.0
  - @pnpm/default-reporter@8.5.7
  - @pnpm/cli-meta@2.0.1
  - @pnpm/manifest-utils@2.1.6
  - @pnpm/package-is-installable@5.0.11
  - @pnpm/read-project-manifest@2.0.11

## 0.6.43

### Patch Changes

- Updated dependencies [ea24c69fe]
  - @pnpm/default-reporter@8.5.6

## 0.6.42

### Patch Changes

- Updated dependencies [8fe8f5e55]
  - @pnpm/config@13.9.0
  - @pnpm/default-reporter@8.5.5

## 0.6.41

### Patch Changes

- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/config@13.8.0
  - @pnpm/default-reporter@8.5.4

## 0.6.40

### Patch Changes

- @pnpm/default-reporter@8.5.3
- @pnpm/cli-meta@2.0.0
- @pnpm/config@13.7.2
- @pnpm/manifest-utils@2.1.5
- @pnpm/package-is-installable@5.0.10
- @pnpm/read-project-manifest@2.0.10

## 0.6.39

### Patch Changes

- @pnpm/default-reporter@8.5.2

## 0.6.38

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@13.7.1
- @pnpm/default-reporter@8.5.1
- @pnpm/manifest-utils@2.1.4
- @pnpm/package-is-installable@5.0.9
- @pnpm/read-project-manifest@2.0.9

## 0.6.37

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
  - @pnpm/default-reporter@8.5.0
  - @pnpm/config@13.7.0
  - @pnpm/manifest-utils@2.1.3
  - @pnpm/package-is-installable@5.0.8
  - @pnpm/cli-meta@2.0.0
  - @pnpm/read-project-manifest@2.0.8

## 0.6.36

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/config@13.6.1
  - @pnpm/default-reporter@8.4.2

## 0.6.35

### Patch Changes

- Updated dependencies [8a99a01ff]
  - @pnpm/config@13.6.0
  - @pnpm/default-reporter@8.4.1

## 0.6.34

### Patch Changes

- Updated dependencies [597a28e3c]
  - @pnpm/default-reporter@8.4.0

## 0.6.33

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1
  - @pnpm/default-reporter@8.3.8

## 0.6.32

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/config@13.5.0
  - @pnpm/default-reporter@8.3.7

## 0.6.31

### Patch Changes

- Updated dependencies [783cc1051]
  - @pnpm/package-is-installable@5.0.7

## 0.6.30

### Patch Changes

- @pnpm/config@13.4.2
- @pnpm/cli-meta@2.0.0
- @pnpm/default-reporter@8.3.6
- @pnpm/manifest-utils@2.1.2
- @pnpm/package-is-installable@5.0.6
- @pnpm/read-project-manifest@2.0.7

## 0.6.29

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@13.4.1
- @pnpm/default-reporter@8.3.5
- @pnpm/manifest-utils@2.1.1
- @pnpm/package-is-installable@5.0.5
- @pnpm/read-project-manifest@2.0.6

## 0.6.28

### Patch Changes

- Updated dependencies [b6d74c545]
- Updated dependencies [7a021932f]
  - @pnpm/config@13.4.0
  - @pnpm/default-reporter@8.3.4

## 0.6.27

### Patch Changes

- Updated dependencies [bd7bcdbe8]
  - @pnpm/config@13.3.0
  - @pnpm/default-reporter@8.3.3

## 0.6.26

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0
  - @pnpm/default-reporter@8.3.2

## 0.6.25

### Patch Changes

- Updated dependencies [cd597bdf9]
  - @pnpm/default-reporter@8.3.1

## 0.6.24

### Patch Changes

- Updated dependencies [ef9d2719a]
- Updated dependencies [4027a3c69]
  - @pnpm/default-reporter@8.3.0
  - @pnpm/config@13.1.0

## 0.6.23

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0
  - @pnpm/default-reporter@8.2.3

## 0.6.22

### Patch Changes

- Updated dependencies [553a5d840]
- Updated dependencies [d62259d67]
  - @pnpm/manifest-utils@2.1.0
  - @pnpm/config@12.6.0
  - @pnpm/default-reporter@8.2.2

## 0.6.21

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0
  - @pnpm/default-reporter@8.2.1

## 0.6.20

### Patch Changes

- Updated dependencies [e0aa55140]
  - @pnpm/default-reporter@8.2.0

## 0.6.19

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/default-reporter@8.1.14

## 0.6.18

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/default-reporter@8.1.13

## 0.6.17

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/default-reporter@8.1.12

## 0.6.16

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/default-reporter@8.1.11

## 0.6.15

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/default-reporter@8.1.10

## 0.6.14

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/default-reporter@8.1.9

## 0.6.13

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@12.4.3
- @pnpm/default-reporter@8.1.8
- @pnpm/manifest-utils@2.0.4
- @pnpm/package-is-installable@5.0.4
- @pnpm/read-project-manifest@2.0.5

## 0.6.12

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2
  - @pnpm/default-reporter@8.1.7

## 0.6.11

### Patch Changes

- Updated dependencies [67c6a67f9]
  - @pnpm/default-reporter@8.1.6

## 0.6.10

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1
  - @pnpm/default-reporter@8.1.5

## 0.6.9

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/default-reporter@8.1.4

## 0.6.8

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@12.3.3
- @pnpm/default-reporter@8.1.3
- @pnpm/manifest-utils@2.0.3
- @pnpm/package-is-installable@5.0.3
- @pnpm/read-project-manifest@2.0.4

## 0.6.7

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@12.3.2
- @pnpm/default-reporter@8.1.2
- @pnpm/manifest-utils@2.0.2
- @pnpm/package-is-installable@5.0.2
- @pnpm/read-project-manifest@2.0.3

## 0.6.6

### Patch Changes

- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/default-reporter@8.1.1

## 0.6.5

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0
  - @pnpm/default-reporter@8.1.0

## 0.6.4

### Patch Changes

- Updated dependencies [e4a981c0c]
  - @pnpm/default-reporter@8.0.3

## 0.6.3

### Patch Changes

- @pnpm/read-project-manifest@2.0.2
- @pnpm/config@12.2.0

## 0.6.2

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [6e9c112af]
  - @pnpm/config@12.2.0
  - @pnpm/read-project-manifest@2.0.1
  - @pnpm/default-reporter@8.0.2
  - @pnpm/cli-meta@2.0.0
  - @pnpm/manifest-utils@2.0.1
  - @pnpm/package-is-installable@5.0.1

## 0.6.1

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0
  - @pnpm/default-reporter@8.0.1

## 0.6.0

### Minor Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [aed712455]
- Updated dependencies [aed712455]
- Updated dependencies [90487a3a8]
  - @pnpm/cli-meta@2.0.0
  - @pnpm/config@12.0.0
  - @pnpm/default-reporter@8.0.0
  - @pnpm/error@2.0.0
  - @pnpm/manifest-utils@2.0.0
  - @pnpm/package-is-installable@5.0.0
  - @pnpm/read-project-manifest@2.0.0

## 0.5.4

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2
  - @pnpm/default-reporter@7.10.7

## 0.5.3

### Patch Changes

- Updated dependencies [4b3852c39]
  - @pnpm/config@11.14.1
  - @pnpm/default-reporter@7.10.6

## 0.5.2

### Patch Changes

- @pnpm/config@11.14.0
- @pnpm/default-reporter@7.10.5

## 0.5.1

### Patch Changes

- 3be2b1773: Fix URL to CLI docs.

## 0.5.0

### Minor Changes

- cb040ae18: add option to check unknown settings

### Patch Changes

- Updated dependencies [cb040ae18]
  - @pnpm/config@11.14.0
  - @pnpm/default-reporter@7.10.4

## 0.4.51

### Patch Changes

- Updated dependencies [ad113645b]
- Updated dependencies [c4cc62506]
  - @pnpm/read-project-manifest@1.1.7
  - @pnpm/config@11.13.0
  - @pnpm/default-reporter@7.10.3

## 0.4.50

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1
  - @pnpm/default-reporter@7.10.2

## 0.4.49

### Patch Changes

- Updated dependencies [4420f9f4e]
  - @pnpm/default-reporter@7.10.1

## 0.4.48

### Patch Changes

- Updated dependencies [43de80034]
  - @pnpm/cli-meta@1.0.2

## 0.4.47

### Patch Changes

- 548f28df9: Format the printed warnings.
- Updated dependencies [548f28df9]
- Updated dependencies [548f28df9]
  - @pnpm/default-reporter@7.10.0
  - @pnpm/config@11.12.0
  - @pnpm/cli-meta@1.0.1
  - @pnpm/manifest-utils@1.1.5
  - @pnpm/package-is-installable@4.0.19
  - @pnpm/read-project-manifest@1.1.6

## 0.4.46

### Patch Changes

- @pnpm/config@11.11.1

## 0.4.45

### Patch Changes

- Updated dependencies [f40bc5927]
  - @pnpm/config@11.11.0

## 0.4.44

### Patch Changes

- Updated dependencies [425c7547d]
  - @pnpm/config@11.10.2

## 0.4.43

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1

## 0.4.42

### Patch Changes

- Updated dependencies [a8656b42f]
  - @pnpm/config@11.10.0

## 0.4.41

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1

## 0.4.40

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0

## 0.4.39

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0

## 0.4.38

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/config@11.7.2
  - @pnpm/manifest-utils@1.1.4
  - @pnpm/package-is-installable@4.0.18
  - @pnpm/read-project-manifest@1.1.5

## 0.4.37

### Patch Changes

- @pnpm/read-project-manifest@1.1.4

## 0.4.36

### Patch Changes

- @pnpm/read-project-manifest@1.1.3

## 0.4.35

### Patch Changes

- @pnpm/cli-meta@1.0.1
- @pnpm/config@11.7.1
- @pnpm/manifest-utils@1.1.3
- @pnpm/package-is-installable@4.0.17
- @pnpm/read-project-manifest@1.1.2

## 0.4.34

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0

## 0.4.33

### Patch Changes

- @pnpm/cli-meta@1.0.1
- @pnpm/config@11.6.1
- @pnpm/manifest-utils@1.1.2
- @pnpm/package-is-installable@4.0.16
- @pnpm/read-project-manifest@1.1.1

## 0.4.32

### Patch Changes

- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0

## 0.4.31

### Patch Changes

- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 0.4.30

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0

## 0.4.29

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0

## 0.4.28

### Patch Changes

- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0

## 0.4.27

### Patch Changes

- @pnpm/manifest-utils@1.1.1
- @pnpm/package-is-installable@4.0.15

## 0.4.26

### Patch Changes

- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
  - @pnpm/manifest-utils@1.1.0

## 0.4.25

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7
  - @pnpm/package-is-installable@4.0.14
  - @pnpm/read-project-manifest@1.0.13

## 0.4.24

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6

## 0.4.23

### Patch Changes

- Updated dependencies [972864e0d]
  - @pnpm/config@11.2.5

## 0.4.22

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/config@11.2.4
  - @pnpm/package-is-installable@4.0.13
  - @pnpm/read-project-manifest@1.0.12

## 0.4.21

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3

## 0.4.20

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2

## 0.4.19

### Patch Changes

- @pnpm/read-project-manifest@1.0.11

## 0.4.18

### Patch Changes

- Updated dependencies [3bd3253e3]
  - @pnpm/read-project-manifest@1.0.10

## 0.4.17

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/cli-meta@1.0.1
  - @pnpm/config@11.2.1

## 0.4.16

### Patch Changes

- ad69677a7: A new option added that allows to resolve the global bin directory from directories to which there is no write access.
- Updated dependencies [ad69677a7]
  - @pnpm/config@11.2.0

## 0.4.15

### Patch Changes

- @pnpm/package-is-installable@4.0.12

## 0.4.14

### Patch Changes

- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/config@11.1.0

## 0.4.13

### Patch Changes

- @pnpm/config@11.0.1

## 0.4.12

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0

## 0.4.11

### Patch Changes

- @pnpm/config@10.0.1

## 0.4.10

### Patch Changes

- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
  - @pnpm/config@10.0.0
  - @pnpm/cli-meta@1.0.0
  - @pnpm/manifest-utils@1.0.3
  - @pnpm/package-is-installable@4.0.11
  - @pnpm/read-project-manifest@1.0.9

## 0.4.9

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/config@9.2.0
  - @pnpm/cli-meta@1.0.0
  - @pnpm/manifest-utils@1.0.2
  - @pnpm/package-is-installable@4.0.10
  - @pnpm/read-project-manifest@1.0.8

## 0.4.8

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.
- Updated dependencies [57c510f00]
  - @pnpm/read-project-manifest@1.0.7

## 0.4.7

### Patch Changes

- @pnpm/package-is-installable@4.0.9

## 0.4.6

### Patch Changes

- Updated dependencies [ffddf34a8]
  - @pnpm/config@9.1.0

## 0.4.5

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0
  - @pnpm/cli-meta@1.0.0
  - @pnpm/error@1.2.1
  - @pnpm/manifest-utils@1.0.1
  - @pnpm/package-is-installable@4.0.8
  - @pnpm/read-project-manifest@1.0.6

## 0.4.5-alpha.2

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2

## 0.4.5-alpha.1

### Patch Changes

- @pnpm/cli-meta@1.0.0-alpha.0
- @pnpm/config@8.3.1-alpha.1
- @pnpm/manifest-utils@1.0.1-alpha.0
- @pnpm/package-is-installable@4.0.8-alpha.0
- @pnpm/read-project-manifest@1.0.6-alpha.0

## 0.4.5-alpha.0

### Patch Changes

- @pnpm/config@8.3.1-alpha.0

## 0.4.4

### Patch Changes

- @pnpm/read-project-manifest@1.0.5
