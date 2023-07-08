---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Ensure consistent output for scripts executed concurrently, both within a single project and across multiple projects. Each script's output will now be printed in a separate section of the terminal, when running multiple scripts in a single project [using regex](https://pnpm.io/cli/run#running-multiple-scripts) [#6692](https://github.com/pnpm/pnpm/issues/6692).
