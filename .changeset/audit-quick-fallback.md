---
"@pnpm/audit": patch
"pnpm": patch
---

Use the `/-/npm/v1/security/audits/quick` endpoint as the primary audit endpoint, falling back to `/-/npm/v1/security/audits` when it fails [#10649](https://github.com/pnpm/pnpm/issues/10649).
