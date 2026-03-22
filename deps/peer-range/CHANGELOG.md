# @pnpm/semver.peer-range

## 1000.0.0

### Major Changes

- e8c2b17: Prevent `overrides` from adding invalid version ranges to `peerDependencies` by keeping the `peerDependencies` and overriding them with prod `dependencies` [#8978](https://github.com/pnpm/pnpm/issues/8978).
