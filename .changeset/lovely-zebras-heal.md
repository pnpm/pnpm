---
"@pnpm/config": minor
"@pnpm/plugin-commands-script-runners": minor
"pnpm": minor
---

Add new global option called `--stream`.
When used, the output from child processes is streamed to the console immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.
