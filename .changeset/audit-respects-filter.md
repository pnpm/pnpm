---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` and `pnpm audit signatures` now check only the dependencies of the projects selected by `--filter`, `--filter-prod`, or `--workspace-root`. The filter used to be ignored, so a filtered audit reported the whole workspace [#10982](https://github.com/pnpm/pnpm/issues/10982).
