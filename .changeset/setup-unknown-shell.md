---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm setup` no longer fails when it cannot tell which shell configuration file to update, for example in a container where `SHELL` is not set. It installs pnpm and prints the `PNPM_HOME` and `PATH` lines to add yourself. The installer script at get.pnpm.io no longer fails in such containers either [#16973](https://github.com/pnpm/pnpm/issues/16973).
