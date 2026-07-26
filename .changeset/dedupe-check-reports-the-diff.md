---
"pacquet": patch
---

`pnpm dedupe --check` now reports what deduplication would change: the importer and package snapshot diff, the `ERR_PNPM_DEDUPE_CHECK_ISSUES` error code, and the warning that points at `pnpm peers check` when the install leaves peer-dependency issues behind. `pnpm peers check` is also accepted again — the subcommand spelling used on pnpm.io and in pnpm's own dedupe output — instead of failing with "unexpected argument 'check' found" [#13321](https://github.com/pnpm/pnpm/issues/13321).
