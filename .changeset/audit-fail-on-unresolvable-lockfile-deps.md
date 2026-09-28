---
"@pnpm/deps.compliance.audit": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` and `pnpm audit signatures` now fail with an error when the lockfile contains unresolvable dependency references [pnpm/pnpm#13638](https://github.com/pnpm/pnpm/issues/13638).
