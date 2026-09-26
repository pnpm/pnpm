---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

On Windows, pnpm now fails within about a second when it cannot move a `node_modules` directory installed by another package manager because a file in it is in use. The error names the directory and suggests stopping the process that uses it. pnpm used to retry for a minute and then print a raw `EPERM` stack trace [#7505](https://github.com/pnpm/pnpm/issues/7505).
