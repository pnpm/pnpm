---
"@pnpm/workspace.state": patch
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now links the runtime that an earlier `pnpm install --no-runtime` skipped [pnpm/pnpm#17542](https://github.com/pnpm/pnpm/issues/17542).
