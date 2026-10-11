---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm setup` no longer tries to install pnpm globally from npm's `bin` directory when pnpm was installed with npm. It failed with `Failed to create bin at <PNPM_HOME>/bin/pnpm` [#16855](https://github.com/pnpm/pnpm/issues/16855).
