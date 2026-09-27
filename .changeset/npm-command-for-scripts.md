---
"pacquet": patch
---

Scripts now see the `npm_command` environment variable that pnpm 11 and npm set. It holds `run-script` when the command runs a script, and the command's own name otherwise [#16265](https://github.com/pnpm/pnpm/issues/16265).
