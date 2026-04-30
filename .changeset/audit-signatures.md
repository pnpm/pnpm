---
"@pnpm/deps.compliance.audit": minor
"@pnpm/deps.compliance.commands": minor
"pnpm": minor
---

Added `pnpm audit signatures` to verify ECDSA registry signatures for installed packages against keys from `/-/npm/v1/keys` [#7909](https://github.com/pnpm/pnpm/issues/7909). Scoped registries are respected, and registries without signing keys are skipped.
