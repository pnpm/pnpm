# @pnpm/node.fetcher

## 2.0.1

### Patch Changes

- @pnpm/fetcher-base@13.1.3
- @pnpm/tarball-fetcher@12.0.1
- @pnpm/create-cafs-store@3.0.1
- @pnpm/pick-fetcher@1.0.0

## 2.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/create-cafs-store@3.0.0
  - @pnpm/error@4.0.0
  - @pnpm/tarball-fetcher@12.0.0

## 1.0.15

### Patch Changes

- @pnpm/tarball-fetcher@11.0.5
- @pnpm/create-cafs-store@2.2.5

## 1.0.14

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/tarball-fetcher@11.0.4

## 1.0.13

### Patch Changes

- @pnpm/fetcher-base@13.1.2
- @pnpm/tarball-fetcher@11.0.3
- @pnpm/create-cafs-store@2.2.4
- @pnpm/pick-fetcher@1.0.0

## 1.0.12

### Patch Changes

- @pnpm/fetcher-base@13.1.1
- @pnpm/tarball-fetcher@11.0.2
- @pnpm/create-cafs-store@2.2.3
- @pnpm/pick-fetcher@1.0.0

## 1.0.11

### Patch Changes

- 1c7b439bb: For node version < 16, install x64 build on darwin arm as arm build is not available.

## 1.0.10

### Patch Changes

- @pnpm/create-cafs-store@2.2.2
- @pnpm/fetcher-base@13.1.0
- @pnpm/tarball-fetcher@11.0.1

## 1.0.9

### Patch Changes

- Updated dependencies [dbac0ca01]
  - @pnpm/tarball-fetcher@11.0.1
  - @pnpm/create-cafs-store@2.2.1

## 1.0.8

### Patch Changes

- 32915f0e4: Refactor cafs types into separate package and add additional properties including `cafsDir` and `getFilePathInCafs`.
- 7a17f99ab: Refactor `tarball-fetcher` and separate it into more specific fetchers, such as `localTarball`, `remoteTarball` and `gitHostedTarball`.
- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
- Updated dependencies [7a17f99ab]
  - @pnpm/create-cafs-store@2.2.0
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/tarball-fetcher@11.0.0
  - @pnpm/pick-fetcher@1.0.0

## 1.0.7

### Patch Changes

- @pnpm/create-cafs-store@2.1.1
- @pnpm/tarball-fetcher@10.0.10

## 1.0.6

### Patch Changes

- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/tarball-fetcher@10.0.10
  - @pnpm/create-cafs-store@2.1.0

## 1.0.5

### Patch Changes

- @pnpm/fetcher-base@13.0.2
- @pnpm/tarball-fetcher@10.0.9
- @pnpm/create-cafs-store@2.0.3

## 1.0.4

### Patch Changes

- 2105735a0: `pnpm env use` should throw an error on a system that use the MUSL libc.

## 1.0.3

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/tarball-fetcher@10.0.8

## 1.0.2

### Patch Changes

- @pnpm/create-cafs-store@2.0.2
- @pnpm/tarball-fetcher@10.0.7

## 1.0.1

### Patch Changes

- @pnpm/fetcher-base@13.0.1
- @pnpm/create-cafs-store@2.0.1
- @pnpm/tarball-fetcher@10.0.7

## 1.0.0

### Major Changes

- 228dcc3c9: Initial release.

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/create-cafs-store@2.0.0
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/tarball-fetcher@10.0.6
