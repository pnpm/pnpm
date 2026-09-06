---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` no longer counts advisories suppressed by `auditConfig` in its summary. The headline total and the `Severity:` breakdown now describe the same advisories the exit code is based on. Suppressed advisories are reported on their own line, `2 ignored: 1 moderate | 1 critical`. A run whose only advisory was suppressed printed a red `1 vulnerabilities found` next to a zero exit code [#14535](https://github.com/pnpm/pnpm/issues/14535).
