---
"pacquet": patch
---

pnpm now warns when a pnpmfile defines the `importPackage` hook. pnpm does not run this hook. Use the `packageImportMethod` setting to choose how packages are written to `node_modules`.
