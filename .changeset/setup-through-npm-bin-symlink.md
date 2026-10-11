---
"pacquet": patch
---

`pnpm setup` now works when pnpm was installed with npm on macOS. It installed pnpm from npm's `bin` directory and left a `$PNPM_HOME/bin/pnpm` that could not run [#16855](https://github.com/pnpm/pnpm/issues/16855).
