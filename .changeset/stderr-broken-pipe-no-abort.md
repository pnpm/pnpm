---
"pacquet": patch
---

pnpm now keeps running when a write to stderr fails because the pipe's reader has exited. A failing script run as `pnpm start 2>&1 | head -1` exits with the script's exit code. Previously the failed write aborted pnpm with a core dump [#16904](https://github.com/pnpm/pnpm/issues/16904).
