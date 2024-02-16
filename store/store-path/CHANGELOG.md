# @pnpm/store-path

## 8.0.2

### Patch Changes

- 37ccff637: Throw an error when calculating the store directory without the pnpm home directory.

## 8.0.1

### Patch Changes

- 7d65d901a: Fix issue when trying to use `pnpm dlx` in the root of a Windows Drive [#7263](https://github.com/pnpm/pnpm/issues/7263).

## 8.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 7.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 6.0.0

### Major Changes

- cdeb65203: Changed the location of the global store from `~/.pnpm-store` to `<pnpm home directory>/store`

  On Linux, by default it will be `~/.local/share/pnpm/store`
  On Windows: `%LOCALAPPDATA%/pnpm/store`
  On macOS: `~/Library/pnpm/store`

  Related issue: [#2574](https://github.com/pnpm/pnpm/issues/2574)
