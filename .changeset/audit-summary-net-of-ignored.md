---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` no longer counts advisories suppressed by `auditConfig` in its summary [#14535](https://github.com/pnpm/pnpm/issues/14535). The headline total and the per-severity counts now describe the same advisories the exit code is based on, and each severity keeps its `(N ignored)` note. When every advisory is suppressed, the summary reads `No known vulnerabilities found (1 ignored)` instead of reporting a vulnerability alongside a zero exit code.
