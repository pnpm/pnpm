---
"pacquet": patch
---

On Windows, `pnpm exec`, `pnpm run`, and commands such as `pnpm nx` now return when the command they run exits, even if it left a detached process running and pnpm's output is redirected or piped. Previously a shell reading that output waited until the detached process, such as the Nx daemon, exited [#16930](https://github.com/pnpm/pnpm/issues/16930).
