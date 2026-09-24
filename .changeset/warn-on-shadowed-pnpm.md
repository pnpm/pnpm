---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": minor
---

`pnpm self-update` and `pnpm setup` now warn when another `pnpm` on PATH comes before the global bin directory, because the shell would keep running that copy. The warning names the executable, how it was installed (npm, Homebrew, Corepack, Volta, or Scoop), and the command that removes it.
