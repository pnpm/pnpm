---
"pacquet": minor
---

`pnpm doctor` now warns in its "Install method" check when the `pnpm` found on PATH is a copy pnpm did not install, ahead of the global bin directory. The warning names that copy. If pnpm recognizes its installer, it also gives the command that removes it.
