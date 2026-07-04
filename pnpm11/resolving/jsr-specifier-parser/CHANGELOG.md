# @pnpm/resolving.jsr-specifier-parser

## 1100.0.2

### Patch Changes

- 25c7388: pnpm now rejects `jsr:` specifiers whose package name is not a valid npm package name — an empty scope or name (e.g. `jsr:@scope/`), path separators inside the name, or any other shape `validate-npm-package-name` rejects — with `ERR_PNPM_INVALID_JSR_PACKAGE_NAME` instead of silently converting them into a malformed `@jsr/...` npm package name.

## 1100.0.1

### Patch Changes

- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
- Updated dependencies [831f574]
  - @pnpm/error@1001.0.0

## 1000.0.3

### Patch Changes

- @pnpm/error@1000.0.5

## 1000.0.2

### Patch Changes

- @pnpm/error@1000.0.4

## 1000.0.1

### Patch Changes

- @pnpm/error@1000.0.3

## 1000.0.0

### Major Changes

- 9c3dd03: Initial release.
