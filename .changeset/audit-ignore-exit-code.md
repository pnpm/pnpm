---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit --ignore` and `pnpm audit --ignore-unfixable` now exit with a non-zero code when vulnerabilities at or above the audit level remain after ignoring. Before, these commands always exited with code 0.
