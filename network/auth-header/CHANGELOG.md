# @pnpm/network.auth-header

## 3.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
  - @pnpm/error@6.0.0

## 2.2.0

### Minor Changes

- 5a5e42551: Export the loadToken function.

## 2.1.0

### Minor Changes

- 3ac0487b3: Add support for basic authorization header [#7371](https://github.com/pnpm/pnpm/issues/7371).

## 2.0.6

### Patch Changes

- 23039a6d6: Fix missing auth tokens in registries with paths specified (e.g. //npm.pkg.github.com/pnpm). #5970 #2933

## 2.0.5

### Patch Changes

- aa20818a0: Authorization token should be found in the configuration, when the requested URL is explicitly specified with a default port (443 on HTTPS or 80 on HTTP) [#6863](https://github.com/pnpm/pnpm/pull/6864).

## 2.0.4

### Patch Changes

- e44031e71: Improve the performance of searching for auth tokens.

## 2.0.3

### Patch Changes

- 4e7afec90: Ignore the port in the URL, while searching for authentication token in the `.npmrc` file [#6354](https://github.com/pnpm/pnpm/issues/6354).

## 2.0.2

### Patch Changes

- @pnpm/error@5.0.2

## 2.0.1

### Patch Changes

- @pnpm/error@5.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/error@5.0.0

## 1.0.1

### Patch Changes

- @pnpm/error@4.0.1

## 1.0.0

### Major Changes

- 804de211e: Initial release.
