---
"@pnpm/exec.build-commands": patch
"pnpm": patch
---

When executing the `approve-builds` command, if package.json contains `onlyBuiltDependencies` or `ignoredBuiltDependencies`, the selected dependency package will continue to be written into `package.json`.
