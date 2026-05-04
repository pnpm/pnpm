---
"@pnpm/deps.inspection.peers-issues-renderer": minor
"@pnpm/deps.inspection.commands": patch
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

When `strictPeerDependencies` is `true`, the `ERR_PNPM_PEER_DEP_ISSUES` error once again renders the peer dependency issues inline using the same format as `pnpm peers check`, so users (and CI tools like Renovate) can see what failed without running `pnpm peers check` separately [#11439](https://github.com/pnpm/pnpm/issues/11439).
