---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

Don't treat files like `license16.json` as a package license when deciding if the workspace LICENSE file should be included in the packed package.
