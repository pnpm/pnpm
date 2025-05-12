---
"@pnpm/core": patch
"pnpm": patch
---

Installation should not exit with an error if `strictPeerDependencies` is `true` but all issues are ignored by `peerDependencyRules` [#9505](https://github.com/pnpm/pnpm/pull/9505).
