---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` no longer counts advisories suppressed by `auditConfig` in its summary. The headline total and the per-severity counts now describe the same advisories the exit code is based on, and each severity keeps its `(N ignored)` note. A run whose only advisory was suppressed printed a red `1 vulnerabilities found` next to a zero exit code. It now reads `No known vulnerabilities found (1 ignored)` [#14535](https://github.com/pnpm/pnpm/issues/14535).
