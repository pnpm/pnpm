# @pnpm/plugin-commands-env

## 2.1.1

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0
  - @pnpm/cli-utils@0.7.2

## 2.1.0

### Minor Changes

- 8fa95fd86: Path `extraNodePaths` to the bins linker.

### Patch Changes

- Updated dependencies [cdeb65203]
- Updated dependencies [8dac029ef]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
  - @pnpm/store-path@6.0.0
  - @pnpm/config@15.0.0
  - @pnpm/package-store@13.0.1
  - @pnpm/cli-utils@0.7.1
  - @pnpm/error@3.0.1
  - @pnpm/tarball-fetcher@10.0.1

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/error@3.0.0
  - @pnpm/fetch@5.0.0
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/package-store@13.0.0
  - @pnpm/tarball-fetcher@10.0.0
  - @pnpm/cli-utils@0.7.0

## 1.4.14

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/cli-utils@0.6.50
  - @pnpm/config@13.13.2
  - @pnpm/tarball-fetcher@9.3.17
  - @pnpm/package-store@12.1.12

## 1.4.13

### Patch Changes

- @pnpm/package-store@12.1.11
- @pnpm/cli-utils@0.6.49
- @pnpm/config@13.13.1
- @pnpm/fetcher-base@11.1.6
- @pnpm/tarball-fetcher@9.3.16
- @pnpm/fetch@4.2.5

## 1.4.12

### Patch Changes

- Updated dependencies [fa4f9133b]
  - @pnpm/package-store@12.1.10
  - @pnpm/tarball-fetcher@9.3.15

## 1.4.11

### Patch Changes

- Updated dependencies [50e347d23]
  - @pnpm/package-store@12.1.9
  - @pnpm/tarball-fetcher@9.3.15

## 1.4.10

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0
  - @pnpm/cli-utils@0.6.48

## 1.4.9

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0
  - @pnpm/cli-utils@0.6.47

## 1.4.8

### Patch Changes

- Updated dependencies [fff0e4493]
  - @pnpm/config@13.11.0
  - @pnpm/cli-utils@0.6.46

## 1.4.7

### Patch Changes

- @pnpm/cli-utils@0.6.45

## 1.4.6

### Patch Changes

- Updated dependencies [e76151f66]
  - @pnpm/config@13.10.0
  - @pnpm/cli-utils@0.6.44
  - @pnpm/fetcher-base@11.1.5
  - @pnpm/package-store@12.1.8
  - @pnpm/fetch@4.2.4
  - @pnpm/tarball-fetcher@9.3.15

## 1.4.5

### Patch Changes

- @pnpm/cli-utils@0.6.43

## 1.4.4

### Patch Changes

- @pnpm/package-store@12.1.7
- @pnpm/tarball-fetcher@9.3.14

## 1.4.3

### Patch Changes

- Updated dependencies [8fe8f5e55]
  - @pnpm/config@13.9.0
  - @pnpm/cli-utils@0.6.42
  - @pnpm/package-store@12.1.6
  - @pnpm/tarball-fetcher@9.3.14

## 1.4.2

### Patch Changes

- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/config@13.8.0
  - @pnpm/package-store@12.1.6
  - @pnpm/cli-utils@0.6.41

## 1.4.1

### Patch Changes

- @pnpm/cli-utils@0.6.40
- @pnpm/config@13.7.2
- @pnpm/fetcher-base@11.1.4
- @pnpm/package-store@12.1.6
- @pnpm/fetch@4.2.3
- @pnpm/tarball-fetcher@9.3.14

## 1.4.0

### Minor Changes

- d16620cf9: If pnpm previously failed to install node when the `use-node-version` option is set, that download and install will now be re-attempted when pnpm is run again.

### Patch Changes

- @pnpm/cli-utils@0.6.39

## 1.3.1

### Patch Changes

- @pnpm/cli-utils@0.6.38
- @pnpm/config@13.7.1
- @pnpm/fetcher-base@11.1.3
- @pnpm/package-store@12.1.5
- @pnpm/tarball-fetcher@9.3.13
- @pnpm/fetch@4.2.2

## 1.3.0

### Minor Changes

- 10a4bd4db: New option added for: `node-mirror:<releaseDir>`. The string value of this dynamic option is used as the base URL for downloading node when `use-node-version` is specified. The `<releaseDir>` portion of this argument can be any dir in `https://nodejs.org/download`. Which `<releaseDir>` dynamic config option gets selected depends on the value of `use-node-version`. If 'use-node-version' is a simple `x.x.x` version string, `<releaseDir>` becomes `release` and `node-mirror:release` is read. Defaults to `https://nodejs.org/download/<releaseDir>/`.

### Patch Changes

- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
- Updated dependencies [d00e1fc6a]
  - @pnpm/config@13.7.0
  - @pnpm/package-store@12.1.4
  - @pnpm/fetch@4.2.1
  - @pnpm/tarball-fetcher@9.3.12
  - @pnpm/cli-utils@0.6.37
  - @pnpm/fetcher-base@11.1.2

## 1.2.12

### Patch Changes

- Updated dependencies [b13e4b452]
  - @pnpm/tarball-fetcher@9.3.11
  - @pnpm/package-store@12.1.3

## 1.2.11

### Patch Changes

- Updated dependencies [f1c194ded]
- Updated dependencies [46aaf7108]
  - @pnpm/fetch@4.2.0
  - @pnpm/config@13.6.1
  - @pnpm/tarball-fetcher@9.3.10
  - @pnpm/cli-utils@0.6.36
  - @pnpm/package-store@12.1.3

## 1.2.10

### Patch Changes

- Updated dependencies [8a99a01ff]
  - @pnpm/config@13.6.0
  - @pnpm/cli-utils@0.6.35

## 1.2.9

### Patch Changes

- Updated dependencies [fb1a95a6c]
- Updated dependencies [fb1a95a6c]
  - @pnpm/tarball-fetcher@9.3.10
  - @pnpm/cli-utils@0.6.34
  - @pnpm/package-store@12.1.3

## 1.2.8

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1
  - @pnpm/cli-utils@0.6.33
  - @pnpm/package-store@12.1.3
  - @pnpm/tarball-fetcher@9.3.9

## 1.2.7

### Patch Changes

- d4d7c4aee: `pnpm env use` should download the right Node.js tarball on Raspberry Pi [#4007](https://github.com/pnpm/pnpm/issues/4007).
- Updated dependencies [002778559]
- Updated dependencies [12ee3c144]
  - @pnpm/config@13.5.0
  - @pnpm/fetch@4.1.6
  - @pnpm/cli-utils@0.6.32
  - @pnpm/tarball-fetcher@9.3.9
  - @pnpm/package-store@12.1.2

## 1.2.6

### Patch Changes

- @pnpm/cli-utils@0.6.31
- @pnpm/package-store@12.1.2
- @pnpm/tarball-fetcher@9.3.9

## 1.2.5

### Patch Changes

- @pnpm/config@13.4.2
- @pnpm/cli-utils@0.6.30
- @pnpm/fetcher-base@11.1.1
- @pnpm/package-store@12.1.1
- @pnpm/fetch@4.1.5
- @pnpm/tarball-fetcher@9.3.9

## 1.2.4

### Patch Changes

- 6b7eb7249: Use the package manager's network and proxy configuration when making requests for Node.js.
  - @pnpm/package-store@12.1.0

## 1.2.3

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/package-store@12.1.0
  - @pnpm/cli-utils@0.6.29
  - @pnpm/config@13.4.1
  - @pnpm/tarball-fetcher@9.3.8
  - @pnpm/fetch@4.1.4

## 1.2.2

### Patch Changes

- Updated dependencies [782ef2490]
  - @pnpm/fetch@4.1.3
  - @pnpm/tarball-fetcher@9.3.7
  - @pnpm/package-store@12.0.15

## 1.2.1

### Patch Changes

- Updated dependencies [b6d74c545]
  - @pnpm/config@13.4.0
  - @pnpm/cli-utils@0.6.28
  - @pnpm/package-store@12.0.15

## 1.2.0

### Minor Changes

- 37905fcf7: Install prerelease Node.js versions.
- 1a6cc7ee7: Allow to install the latest Node.js version by running `pnpm env use -g latest`.

### Patch Changes

- Updated dependencies [bd7bcdbe8]
  - @pnpm/config@13.3.0
  - @pnpm/fetch@4.1.2
  - @pnpm/cli-utils@0.6.27
  - @pnpm/tarball-fetcher@9.3.7
  - @pnpm/package-store@12.0.15

## 1.1.0

### Minor Changes

- 5ee3b2dc7: `pnpm env use` sets the `globalconfig` for npm CLI. The global config is located in a centralized place, so it persists after switching to a different Node.js or npm version.

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0
  - @pnpm/cli-utils@0.6.26

## 1.0.10

### Patch Changes

- 913d97a05: Do not create a command shim for Node.js, just a symlink to the executable.
  - @pnpm/cli-utils@0.6.25

## 1.0.9

### Patch Changes

- Updated dependencies [4027a3c69]
  - @pnpm/config@13.1.0
  - @pnpm/cli-utils@0.6.24

## 1.0.8

### Patch Changes

- @pnpm/tarball-fetcher@9.3.7
- @pnpm/package-store@12.0.15

## 1.0.7

### Patch Changes

- 0d4a7c69e: Pick the right extension for command files. It is important to write files with .CMD extension on case sensitive Windows drives.
- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0
  - @pnpm/cli-utils@0.6.23

## 1.0.6

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0
  - @pnpm/cli-utils@0.6.22

## 1.0.5

### Patch Changes

- Updated dependencies [6681fdcbc]
- Updated dependencies [bab172385]
  - @pnpm/config@12.5.0
  - @pnpm/fetch@4.1.1
  - @pnpm/cli-utils@0.6.21
  - @pnpm/package-store@12.0.15
  - @pnpm/tarball-fetcher@9.3.6

## 1.0.4

### Patch Changes

- Updated dependencies [eadf0e505]
  - @pnpm/fetch@4.1.0
  - @pnpm/tarball-fetcher@9.3.5
  - @pnpm/cli-utils@0.6.20
  - @pnpm/package-store@12.0.14

## 1.0.3

### Patch Changes

- 869b1afcb: Do not create powershell command shims for node, npm, and npx.

## 1.0.2

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19

## 1.0.1

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18

## 1.0.0

### Major Changes

- 25a2d6e5c: When installing Node.js, also link the npm CLI that is bundled with Node.js.

## 0.2.13

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17
  - @pnpm/package-store@12.0.14
  - @pnpm/tarball-fetcher@9.3.4

## 0.2.12

### Patch Changes

- @pnpm/package-store@12.0.13
- @pnpm/tarball-fetcher@9.3.4

## 0.2.11

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16

## 0.2.10

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/cli-utils@0.6.15

## 0.2.9

### Patch Changes

- 27e6331c6: Allow to install a Node.js version using a semver range.
- af8b5716e: New command added: `pnpm env use --global <version>`. This command installs the specified Node.js version globally.
- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/cli-utils@0.6.14

## 0.2.8

### Patch Changes

- @pnpm/config@12.4.3
- @pnpm/package-store@12.0.12
- @pnpm/fetch@4.0.2
- @pnpm/tarball-fetcher@9.3.4

## 0.2.7

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2

## 0.2.6

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1

## 0.2.5

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/package-store@12.0.11

## 0.2.4

### Patch Changes

- @pnpm/config@12.3.3
- @pnpm/package-store@12.0.11
- @pnpm/fetch@4.0.1
- @pnpm/tarball-fetcher@9.3.3

## 0.2.3

### Patch Changes

- @pnpm/package-store@12.0.10
- @pnpm/tarball-fetcher@9.3.2

## 0.2.2

### Patch Changes

- Updated dependencies [e7d9cd187]
- Updated dependencies [eeff424bd]
  - @pnpm/fetch@4.0.0
  - @pnpm/tarball-fetcher@9.3.2
  - @pnpm/package-store@12.0.9
  - @pnpm/config@12.3.2

## 0.2.1

### Patch Changes

- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/tarball-fetcher@9.3.1
  - @pnpm/package-store@12.0.8

## 0.2.0

### Minor Changes

- c1f137412: Remove the `pnpm node [args...]` command.

### Patch Changes

- 6d2ccc9a3: Download Node.js from nodejs.org, not from the npm registry.
- Updated dependencies [6d2ccc9a3]
  - @pnpm/tarball-fetcher@9.3.0
  - @pnpm/package-store@12.0.7

## 0.1.0

### Minor Changes

- 84ec82e05: Project created.

### Patch Changes

- @pnpm/cli-utils@0.6.5
