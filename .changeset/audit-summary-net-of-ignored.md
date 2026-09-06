---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` no longer counts advisories suppressed by `auditConfig` in its summary. The headline total and the `Severity:` breakdown now count only the advisories that survive the `ignoreGhsas` filter. Suppressed advisories get their own line, `2 ignored: 1 moderate | 1 critical`. A run whose advisories were all suppressed printed a red `1 vulnerabilities found` next to a zero exit code. It now reads `All found vulnerabilities were already reviewed and decided to be ignored` [#14535](https://github.com/pnpm/pnpm/issues/14535).
