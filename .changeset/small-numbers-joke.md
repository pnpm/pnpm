---
"@pnpm/plugin-commands-installation": major
"@pnpm/plugin-commands-publishing": major
"@pnpm/plugin-commands-script-runners": major
"@pnpm/plugin-commands-deploy": major
"@pnpm/plugin-commands-env": major
"@pnpm/workspace.find-packages": major
"@pnpm/constants": major
"@pnpm/core": major
"@pnpm/lifecycle": major
"@pnpm/types": major
"@pnpm/config": major
"pnpm": major
---

Removed support for the `useNodeVersion` and `executionEnv.nodeVersion` fields. `devEngines.runtime` and `engines.runtime` should be used instead [#10373](https://github.com/pnpm/pnpm/pull/10373).
