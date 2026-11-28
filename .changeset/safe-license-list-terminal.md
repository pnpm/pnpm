---
"@pnpm/plugin-commands-licenses": patch
"@pnpm/list": patch
"pnpm": patch
---

`pnpm licenses`, `pnpm list --long`, and `pnpm why --long` now remove terminal control characters from package metadata before printing it.
