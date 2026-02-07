# @pnpm/resolve-workspace-range

## 1000.1.0

### Minor Changes

- ed87c99: Support bare `workspace:` protocol without version specifier. It is now treated as `workspace:*` and resolves to the concrete version during publish [#10436](https://github.com/pnpm/pnpm/pull/10436).

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

## 5.0.1

### Patch Changes

- c0760128d: bump semver to 7.4.0

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 4.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

## 2.1.0

### Minor Changes

- 85fb21a83: Add support for workspace:^ and workspace:~ aliases

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 1.0.2
