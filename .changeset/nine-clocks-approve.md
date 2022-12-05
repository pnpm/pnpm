---
"@pnpm/plugin-commands-audit": patch
"pnpm": patch
---

Vulnerabilities that don't have CVEs codes should not be skipped by `pnpm audit` if an ignoreCves list is declared in `package.json`.
