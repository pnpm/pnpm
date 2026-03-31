---
"@pnpm/installing.deps-resolver": patch
pnpm: patch
---

Fixed a bug in an internal `hoistPeers` function that could cause peer dependencies to be re-resolved instead of locked to existing versions when upgrading packages in rare cases.
