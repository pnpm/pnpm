---
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` exits with the code of a script that handles Ctrl+C and shuts down. A script that finished cleanly is not reported as a lifecycle failure, and the commands after it in the same script still run [pnpm/pnpm#9945](https://github.com/pnpm/pnpm/issues/9945).
