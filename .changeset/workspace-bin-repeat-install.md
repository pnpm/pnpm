---
"@pnpm/deps.status": patch
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now relinks workspace package bins when their declarations change, including with `--frozen-lockfile` and an unchanged lockfile. Filtered installs preserve the need to relink bins in unselected projects [pnpm/pnpm#17324](https://github.com/pnpm/pnpm/issues/17324).
