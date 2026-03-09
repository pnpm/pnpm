---
"@pnpm/run-npm": patch
"pnpm": patch
---

Fixed nested `pnpm` invocations under `pnpm run` failing with "double-loading config" error when commands like `pnpm view` are passed through to npm [#10914](https://github.com/pnpm/pnpm/issues/10914).
