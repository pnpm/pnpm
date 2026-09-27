---
"@pnpm/exec.prepare-package": patch
"pnpm": patch
---

pnpm now uses pnpm to prepare a git-hosted dependency that is a pnpm workspace without a committed lockfile. It used npm before, which could skip the dependency's build [#14011](https://github.com/pnpm/pnpm/issues/14011).
