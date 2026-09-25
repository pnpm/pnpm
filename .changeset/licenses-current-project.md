---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm licenses list` run inside a workspace project now lists only that project's dependencies. Use `--recursive` or `--filter` to list the licenses of other workspace projects [#5689](https://github.com/pnpm/pnpm/issues/5689).
