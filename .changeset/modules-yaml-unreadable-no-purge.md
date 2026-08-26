---
"pacquet": patch
---

An unreadable `node_modules/.modules.yaml` no longer makes `pnpm install` delete `node_modules` and relink every package on each run. The unparsable state file is now reported as an error instead [#14062](https://github.com/pnpm/pnpm/issues/14062).
