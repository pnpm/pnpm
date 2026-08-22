---
"@pnpm/resolving.npm-resolver": minor
"pnpm": minor
"pacquet": minor
---

Added `enginesFiltering` setting (`strict` | `none`, defaulting to `strict` when `engineStrict` is enabled) to filter out engine-incompatible candidate package versions during dependency resolution [pnpm/pnpm#13252](https://github.com/pnpm/pnpm/issues/13252).
