---
"pacquet": patch
---

Recursive runs now start the scripts a `/pattern/` selector matched in one package at the same time. `pnpm --parallel` starts all of them. Other recursive runs keep the number of scripts running at once within `workspaceConcurrency`. The matched scripts previously ran one after another [pnpm/pnpm#14933](https://github.com/pnpm/pnpm/issues/14933).
