---
"pacquet": minor
---

Added the `--force` flag to `pnpm install` and `pnpm add`: optional dependencies whose `cpu` / `os` / `libc` / `engines` don't match the host are installed instead of skipped, and a forced install relinks packages that an earlier install already materialized [#13142](https://github.com/pnpm/pnpm/issues/13142).
