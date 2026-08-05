---
"pacquet": patch
---

`pnpm add` with `--save-dev`, `--save-optional`, or `--save-prod` now moves an already-declared dependency to the target group instead of leaving a duplicate entry in its old group, matching pnpm.
