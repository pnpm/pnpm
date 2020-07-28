---
"@pnpm/plugin-commands-audit": patch
---

`pnpm audit --audit-level high` should not error if the found vulnerabilities are low and/or moderate.
