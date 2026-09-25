---
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/exec.commands": patch
"pnpm": patch
---

`pnpm run --recursive` no longer reports interrupted scripts as lifecycle failures after `Ctrl+C`.
