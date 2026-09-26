---
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` now lists at least one dependency path from every workspace project that depends on a vulnerable package. Before, a project whose dependency was reached through more than 100 paths filled the path list, and other projects that depend on the same package were left out [#12200](https://github.com/pnpm/pnpm/issues/12200).
