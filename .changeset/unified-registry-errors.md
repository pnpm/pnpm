---
"@pnpm/pnpr": patch
---

Registry-owned endpoints now return JSON errors with `error`, `reason`, and stable `ERR_PNPR_` codes. npm error responses and resolution stream errors include these codes too.
