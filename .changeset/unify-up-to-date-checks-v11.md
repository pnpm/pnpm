---
"@pnpm/deps.status": patch
"@pnpm/exec.commands": patch
"pnpm": patch
---

`verifyDepsBeforeRun` now checks whether each importer's resolved catalog versions match the catalog snapshot in `pnpm-lock.yaml`, and respects `lockfileDir` in single-project repositories.

When the install that runs before a script leaves `node_modules` out of date, pnpm now prints a warning.
