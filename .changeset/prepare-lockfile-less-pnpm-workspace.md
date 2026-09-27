---
"@pnpm/exec.prepare-package": patch
"pnpm": patch
---

A git-hosted dependency that is a pnpm workspace without a committed lockfile is now prepared with pnpm. It was installed with npm before, which could skip its build [#14011](https://github.com/pnpm/pnpm/issues/14011).
