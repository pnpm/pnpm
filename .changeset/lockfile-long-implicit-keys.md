---
"pacquet": patch
---

pnpm now reads a lockfile with a mapping key longer than 1,024 characters written as `'key':`, without the `? ` marker. Before, `pnpm install` and other commands failed on such a lockfile with `ERR_PNPM_BROKEN_LOCKFILE`.
