---
"@pnpm/exec.commands": patch
"pnpm": patch
---

Fixed false "Command not found" error on Windows when the command exists but exits with a non-zero exit code [#11000](https://github.com/pnpm/pnpm/issues/11000).
