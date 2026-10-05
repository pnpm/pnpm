---
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.security.signatures": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit signatures` now verifies signatures against the integrity recorded in the lockfile. Packages without a recorded integrity cannot pass signature verification.
