---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now re-verifies lockfile resolutions when the default registry or a scoped registry changes [zkochan/pnpm-tasks#108](https://github.com/zkochan/pnpm-tasks/issues/108).
