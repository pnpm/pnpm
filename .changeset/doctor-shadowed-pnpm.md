---
"pacquet": minor
---

`pnpm doctor` now warns in its "Install method" check when `pnpm` on PATH is not the pnpm in the global bin directory, and names the other copy. If pnpm recognizes its installer, it also gives the command that removes it.
