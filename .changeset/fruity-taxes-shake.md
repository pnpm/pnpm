---
"@pnpm/deps.compliance.commands": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"pnpm": minor
"pacquet": minor
---

Added the `audit.ignorePrune` setting. When set to `true`, `pnpm audit --fix` removes ignored GHSA entries that no longer appear in the audit report.
