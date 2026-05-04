# @pnpm/semver.peer-range

## 1100.0.1

### Patch Changes

- 184ce26: Fix the package name in README.md.

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

## 1000.0.0

### Major Changes

- e8c2b17: Prevent `overrides` from adding invalid version ranges to `peerDependencies` by keeping the `peerDependencies` and overriding them with prod `dependencies` [#8978](https://github.com/pnpm/pnpm/issues/8978).
