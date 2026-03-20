---
"@pnpm/config.reader": minor
"@pnpm/engine.pm.commands": minor
"pnpm": minor
---

Store globally installed binaries in a `bin` subdirectory of `PNPM_HOME` instead of directly in `PNPM_HOME`. This prevents internal directories like `global/` and `store/` from polluting shell autocompletion when `PNPM_HOME` is on PATH [#10986](https://github.com/pnpm/pnpm/issues/10986).

After upgrading, run `pnpm setup` to update your shell configuration.
