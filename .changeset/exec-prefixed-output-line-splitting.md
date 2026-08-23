---
"@pnpm/exec.commands": patch
"pnpm": patch
---

`pnpm exec --recursive --no-reporter-hide-prefix` no longer prints a blank prefixed line after each chunk of a command's output, and no longer splits a line in two when it straddles a chunk boundary.
