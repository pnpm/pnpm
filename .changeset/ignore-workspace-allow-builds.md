---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fixed `pnpm install --ignore-workspace` overwriting the `allowBuilds` map in `pnpm-workspace.yaml`. The ignored builds of a package with a build script were auto-populated into `allowBuilds` even though `--ignore-workspace` was passed, clobbering committed `true`/`false` values with the `set this to true or false` placeholder [#12469](https://github.com/pnpm/pnpm/issues/12469).
