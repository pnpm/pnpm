---
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit --prod` and `pnpm audit --dev` no longer report a package that is only there to satisfy another package's peer dependency when it belongs to the excluded dependency type — for example, an optional peer satisfied by a devDependency is no longer reported under `--prod` [#13605](https://github.com/pnpm/pnpm/issues/13605). Auto-installed peers are still audited.
