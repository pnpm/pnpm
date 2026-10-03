---
"@pnpm/cli.meta": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` now fails with a hint to run `brew upgrade pnpm` when Homebrew installed pnpm. It used to install a second copy of pnpm that the Homebrew one kept shadowing [#16547](https://github.com/pnpm/pnpm/issues/16547).
