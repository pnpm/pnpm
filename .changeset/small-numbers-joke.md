---
"@pnpm/installing.commands": major
"@pnpm/releasing.commands": major
"@pnpm/exec.commands": major
"@pnpm/engine.runtime.commands": major
"@pnpm/workspace.project-finder": major
"@pnpm/constants": major
"@pnpm/installing.deps-installer": major
"@pnpm/exec.lifecycle": major
"@pnpm/types": major
"@pnpm/config.reader": major
"pnpm": major
---

Removed support for the `useNodeVersion` and `executionEnv.nodeVersion` fields. `devEngines.runtime` and `engines.runtime` should be used instead [#10373](https://github.com/pnpm/pnpm/pull/10373).
