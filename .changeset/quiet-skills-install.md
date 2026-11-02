---
"pacquet": patch
---

`pnpm install` and `pnpm ci` now succeed when dependency skills are approved but no agent skill directory exists. Skills remain unlinked until a directory is available. Fixes https://github.com/pnpm/pnpm/issues/17574.
