---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

A named-registry dependency whose scoped alias points at an unscoped package, such as `"@local/dep": "work:dep-a@1.2.3"`, now installs `dep-a` from that registry. pnpm used to read `dep-a@1.2.3` as a dist-tag of `@local/dep` and fail.
