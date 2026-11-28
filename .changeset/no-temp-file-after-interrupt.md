---
"pnpm": patch
---

Interrupting pnpm with `Ctrl+C` no longer leaves a temporary file next to a file pnpm was starting to write, such as the lockfile.
