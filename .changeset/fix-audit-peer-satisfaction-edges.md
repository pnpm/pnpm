---
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm audit --prod`/`--dev` incorrectly reporting (and misclassifying) a package that only entered the resolved dependency graph because it was the concrete package chosen to satisfy another package's peer dependency, when that concrete package itself came from an excluded dependency type — for example, an optional peer satisfied by a devDependency, still reported under `--prod` [https://github.com/pnpm/pnpm/issues/13605](https://github.com/pnpm/pnpm/issues/13605).
