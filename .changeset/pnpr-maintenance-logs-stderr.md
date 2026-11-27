---
"@pnpm/pnpr": patch
---

`pnpr` commands such as `pnpr grants export` and `pnpr registries export` now log to stderr. A redirected export no longer starts with a log line, so `pnpr grants import` and `pnpr registries import` accept the file.
