---
"@pnpm/pnpr": patch
---

The access log record under `pnpr::access` now prints each field once. The pretty format printed every field twice and `user` three times, and the JSON format repeated them under `span` [pnpm/tasks#195](https://github.com/pnpm/tasks/issues/195).
