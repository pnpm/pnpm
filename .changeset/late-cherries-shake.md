---
"@pnpm/plugin-commands-outdated": major
---

Return `Promise&lt;{ output: string, exitCode: number }>` instead of `Promise&lt;string>`.

`exitCode` is `1` when there are any outdated packages in the dependencies.
