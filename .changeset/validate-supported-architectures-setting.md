---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reports an `INVALID_SETTING` error when `supportedArchitectures` or any of its nested fields (`os`, `cpu`, `libc`) in `pnpm-workspace.yaml` has an invalid type.
