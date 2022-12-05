---
"@pnpm/plugin-commands-audit": patch
"pnpm": patch
---

`pnpm audit --json` should ignore vulnerabilities listed in `auditConfig.ignoreCves` [#5734](https://github.com/pnpm/pnpm/issues/5734).
