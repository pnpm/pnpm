---
"@pnpm/config.reader": patch
"@pnpm/exec.commands": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).
