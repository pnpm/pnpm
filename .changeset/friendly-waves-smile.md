---
"@pnpm/config": patch
---

Do not crash when an environment variable referenced in `pnpm-workspace.yaml` is not defined. Instead, a warning is logged and the original value is preserved.
