---
"@pnpm/audit": patch
"pnpm": patch
---

Fallback to the `/-/npm/v1/security/audits/quick` endpoint when the default `/-/npm/v1/security/audits` endpoint fails. This fixes `pnpm audit` failures caused by registry 5xx responses from the primary audit endpoint [#10649](https://github.com/pnpm/pnpm/issues/10649).
