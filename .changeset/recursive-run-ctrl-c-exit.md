---
"pacquet": patch
---

Pressing Ctrl+C during `pnpm -r run` or `pnpm --filter <pkg> run` no longer reports `ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL`. pnpm now ends with the interrupted script's status, such as 130 for a script that SIGINT ended on Linux or macOS [#16964](https://github.com/pnpm/pnpm/issues/16964).
