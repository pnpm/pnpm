---
"@pnpm/deps.compliance.license-scanner": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm licenses list --json` now includes every installed copy of a package in its `paths` array. Multiple copies of the same version previously contributed only one path. This includes hoisted copies and isolated installations with different peer dependencies.
