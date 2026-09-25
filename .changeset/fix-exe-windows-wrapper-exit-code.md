---
"@pnpm/exe": patch
"pnpm": patch
---

Fallback `.cmd` and `.ps1` Windows wrappers in `@pnpm/exe` now propagate the exit status of the invoked `pnpm` command [pnpm/pnpm#14826](https://github.com/pnpm/pnpm/issues/14826).
