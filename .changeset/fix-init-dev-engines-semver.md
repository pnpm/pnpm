---
"@pnpm/workspace.commands": patch
"pnpm": patch
---

`pnpm init` now writes the exact pnpm version to `devEngines.packageManager` instead of a `^` range. Corepack only accepts an exact version there, so it rejected the generated `package.json` with "expected a semver version" [pnpm/pnpm#13969](https://github.com/pnpm/pnpm/issues/13969).
