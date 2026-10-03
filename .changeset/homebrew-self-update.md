---
"@pnpm/cli.meta": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` now fails when Homebrew installed pnpm, and prints the `brew upgrade` command for the installed formula, such as `brew upgrade pnpm` or `brew upgrade pnpm@11`. It used to install a second copy of pnpm that the Homebrew one kept shadowing [#16547](https://github.com/pnpm/pnpm/issues/16547).
