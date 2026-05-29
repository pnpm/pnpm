---
"@pnpm/resolving.registry.types": minor
"@pnpm/resolving.npm-resolver": minor
"pnpm": minor
---

Staged publishes are now recognized in the trust scale. When a package version's registry metadata carries an `approver` field, it is treated as the strongest trust evidence (ranked above trusted publishers and provenance attestations), since staged publishes require 2FA publish approvals. This prevents false-positive trust downgrade errors when moving from a staged publish to a lower trust level [#11887](https://github.com/pnpm/pnpm/issues/11887).
