---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer fails with `ENOTEMPTY` when several installs import the same package into one project at the same time. A package directory that a concurrent install already completed with the same files is kept [pnpm/tasks#48](https://github.com/pnpm/tasks/issues/48).
