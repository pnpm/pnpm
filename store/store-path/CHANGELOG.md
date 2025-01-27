# @pnpm/store-path

## 1000.0.2

### Patch Changes

- Updated dependencies [9a44e6c]
  - @pnpm/constants@1001.1.0
  - @pnpm/error@1000.0.2

## 1000.0.1

### Patch Changes

- Updated dependencies [d2e83b0]
- Updated dependencies [a76da0c]
  - @pnpm/constants@1001.0.0
  - @pnpm/error@1000.0.1

## 9.0.3

### Patch Changes

- Updated dependencies [19d5b51]
- Updated dependencies [8108680]
- Updated dependencies [c4f5231]
  - @pnpm/constants@10.0.0
  - @pnpm/error@6.0.3

## 9.0.2

### Patch Changes

- @pnpm/error@6.0.2

## 9.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 9.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
  - @pnpm/error@6.0.0

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
