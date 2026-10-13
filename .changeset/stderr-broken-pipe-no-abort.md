---
"pacquet": patch
---

pnpm no longer aborts with a core dump when it writes to a stderr pipe whose reader has exited, for example when a failing script runs as `pnpm start 2>&1 | head -1`. It now exits with the script's exit code [#16904](https://github.com/pnpm/pnpm/issues/16904).
