---
"pacquet": minor
---

`pnpm self-update` and `pnpm setup` now warn when another `pnpm` on PATH comes before the global bin directory, because the shell would keep running that copy. The warning names that copy. If pnpm recognizes how it was installed (npm, Homebrew, Corepack, Volta, or Scoop), the warning also gives the command that removes it.
