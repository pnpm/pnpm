---
"@pnpm/lifecycle": patch
"pnpm": patch
---

Canceling a running process with Ctrl-C should make `pnpm run` return a non-zero exit code [#9626](https://github.com/pnpm/pnpm/issues/9626).
