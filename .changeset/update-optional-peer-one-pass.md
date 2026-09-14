---
"pacquet": patch
---

`pnpm update` now settles the lockfile in one run when an upgrade removes the package that provided an optional peer dependency. A second `pnpm update` used to change the lockfile again, with no version change in the diff [#14895](https://github.com/pnpm/pnpm/issues/14895).
