---
"pacquet": patch
---

pnpm now passes Ctrl+C on to the script or command it started and waits for it to shut down. pnpm used to exit first, so a script that was still writing landed on the shell prompt [#14723](https://github.com/pnpm/pnpm/issues/14723).
