# @pnpm/deps.inspection.peers-issues-renderer

## 1100.0.0

### Minor Changes

- 0b2f86e: When `strictPeerDependencies` is `true`, the `ERR_PNPM_PEER_DEP_ISSUES` error once again renders the peer dependency issues inline using the same format as `pnpm peers check`, so users (and CI tools like Renovate) can see what failed without running `pnpm peers check` separately [#11439](https://github.com/pnpm/pnpm/issues/11439).
