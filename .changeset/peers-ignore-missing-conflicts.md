---
"@pnpm/deps.inspection.peers-checker": patch
"pnpm": patch
---

`pnpm peers` no longer reports a conflict for a missing peer dependency that is ignored via `pnpm.peerDependencyRules.ignoreMissing`.
