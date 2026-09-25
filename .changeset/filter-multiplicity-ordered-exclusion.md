---
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
"pacquet": patch
---

`--filter` now evaluates selectors in order, so later inclusion filters can re-include packages that an earlier exclusion filter excluded [pnpm/pnpm#9354](https://github.com/pnpm/pnpm/issues/9354).
