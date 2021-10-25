# @pnpm/plugin-commands-env

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
