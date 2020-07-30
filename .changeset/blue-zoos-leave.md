---
"@pnpm/npm-resolver": patch
---

Always use the package name that is given at the root of the metadata object. Override any names that are specified in the version manifests. This fixes an issue with GitHub registry.
