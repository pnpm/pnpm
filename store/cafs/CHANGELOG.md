# @pnpm/store.cafs

## 2.0.7

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/fetcher-base@15.0.3

## 2.0.6

### Patch Changes

- 01bc58e2c: Update ssri to v10.0.5.

## 2.0.5

### Patch Changes

- @pnpm/fetcher-base@15.0.2
- @pnpm/store-controller-types@17.0.1

## 2.0.4

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0
  - @pnpm/graceful-fs@3.2.0
  - @pnpm/fetcher-base@15.0.1

## 2.0.3

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0
  - @pnpm/fetcher-base@15.0.1

## 2.0.2

### Patch Changes

- b3947185c: Tarballs that have hard links are now unpacked successfully. This fixes a regression introduced in v8.7.0, which was shipped with our new in-house tarball parser [#7062](https://github.com/pnpm/pnpm/pull/7062).

## 2.0.1

### Patch Changes

- b548f2f43: Fixes a regression published with pnpm v8.7.3. Don't hang while reading `package.json` from the content-addressable store [#7051](https://github.com/pnpm/pnpm/pull/7051).
- Updated dependencies [4a1a9431d]
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/store-controller-types@16.0.1

## 2.0.0

### Major Changes

- 083bbf590: Breaking changes to the API.

### Patch Changes

- 0fd9e6a6c: Don't prematurely bail out of adding source files if ENOENT is thrown [#6932](https://github.com/pnpm/pnpm/pull/6932).
- Updated dependencies [494f87544]
- Updated dependencies [70b2830ac]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/fetcher-base@15.0.0
  - @pnpm/graceful-fs@3.1.0

## 1.0.2

### Patch Changes

- 73f2b6826: When several containers use the same store simultaneously, there's a chance that multiple containers may create a temporary file at the same time. In such scenarios, pnpm could fail to rename the temporary file in one of the containers. This issue has been addressed: pnpm will no longer fail if the temporary file is absent but the destination file exists.

## 1.0.1

### Patch Changes

- fe1c5f48d: The length of the temporary file names in the content-addressable store reduced in order to prevent `ENAMETOOLONG` errors from happening [#6842](https://github.com/pnpm/pnpm/issues/6842).

## 1.0.0

### Major Changes

- 4bbf482d1: The package is renamed from `@pnpm/cafs` to `@pnpm/store.cafs`.

  The content-addressable store locker should be only created once per process. This fixes an issue that started happening after merging [#6817](https://github.com/pnpm/pnpm/pull/6817)
