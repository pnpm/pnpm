---
"@pnpm/plugin-commands-audit": major
---

Return `Promise&lt;{ output: string, exitCode: number }>` instead of `Promise&lt;string>`.

`exitCode` is `1` when there are any packages with vulnerabilities in the dependencies.
