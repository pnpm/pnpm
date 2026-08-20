---
"@pnpm/installing.deps-installer": patch
"@pnpm/deps.inspection.peers-checker": patch
"pnpm": patch
"pacquet": patch
---

`peerDependencyRules.ignoreMissing` now also suppresses peer dependency errors when the peer is found but doesn't satisfy the required version range, not just when it's absent.
