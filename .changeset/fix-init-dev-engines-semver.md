---
"@pnpm/workspace.commands": patch
"pnpm": patch
---

Fixed `pnpm init` generating an exact `devEngines.packageManager` version instead of a semver range with `^`, avoiding version validation errors in Corepack [pnpm/pnpm#13969](https://github.com/pnpm/pnpm/issues/13969).
