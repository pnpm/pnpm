---
"@pnpm/deps.compliance.commands": patch
---

`pnpm audit --fix` now respects the `auditLevel` setting. Previously, `pnpm audit --fix` would fix all vulnerabilities regardless of the configured `auditLevel`, while `pnpm audit` (without `--fix`) correctly filtered by severity. Now both commands consistently filter advisories by the `auditLevel` setting.
