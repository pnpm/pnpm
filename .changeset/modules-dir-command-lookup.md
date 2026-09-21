---
"@pnpm/config.reader": patch
"@pnpm/exec.commands": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fixed command lookup for custom `modulesDir` settings in `pnpm run` and `pnpm exec`. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).
