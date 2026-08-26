---
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

`--production` is accepted again as an alias of `--prod` on `install`, `fetch`, `prune`, `update`, `list`, `why`, and `sbom`, and the install that `verifyDepsBeforeRun` reproduces is now spelled with `--prod`. `pnpm run` no longer aborts with "unexpected argument '--production' found" after a production-only install [#14147](https://github.com/pnpm/pnpm/issues/14147).
