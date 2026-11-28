---
"pnpm": patch
---

Interrupting pnpm with `Ctrl+C` no longer leaves a temporary file next to a file pnpm was starting to write, such as the lockfile [#1418](https://github.com/pnpm/pnpm/issues/1418).
