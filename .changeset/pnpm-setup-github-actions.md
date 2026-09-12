---
"@pnpm/engine.pm.commands": minor
"pacquet": minor
"pnpm": minor
---

`pnpm setup` now appends `PNPM_HOME` and the global bin directory to the GitHub Actions environment files (`GITHUB_ENV` and `GITHUB_PATH`), so later steps in the same job can run `pnpm add --global` and other global commands [#9191](https://github.com/pnpm/pnpm/issues/9191).
