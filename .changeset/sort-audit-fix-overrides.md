---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

Sort the keys of the overrides object returned by `pnpm audit --fix` so that the log output order matches the order written to `pnpm-workspace.yaml`.
