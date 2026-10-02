---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reports an `INVALID_SETTING` error when top-level `auditLevel` or `auditConfig` in `pnpm-workspace.yaml` has an invalid type or value even when the `audit` section is omitted.
