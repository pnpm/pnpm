---
"pacquet": patch
"@pnpm/pnpr": patch
---

A Python release whose wheel metadata declares a requirement pnpm cannot read no longer fails the install. pnpm now resolves the project against the other releases of that package, and reports the unreadable requirement when none of them works.
