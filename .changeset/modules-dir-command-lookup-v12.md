---
"pacquet": patch
---

Fixed command lookup for a custom `modulesDir` in `pnpm run` and `pnpm exec`. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too. Only a directory name is supported, not a `modulesDir` holding a path separator [#3604](https://github.com/pnpm/pnpm/issues/3604).
