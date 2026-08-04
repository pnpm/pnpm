---
"pacquet": patch
---

`pnpm list --json` and `pnpm list --parseable` now report extraneous packages — packages present in `node_modules` but absent from the lockfile — under `unsavedDependencies`, matching the TypeScript CLI.
