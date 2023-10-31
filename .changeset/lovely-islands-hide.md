---
"@pnpm/config": patch
"@pnpm/core": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolve-dependencies": patch
---

When `use-experimental-npmjs-files-index` is set to `true` requiresBuild in the lockfile from NPM's beta file index feature. This will resolve optional modules being copied rather than linked. 
`use-experimental-npmjs-files-index` can also accept an array of strings, which can be used if a registry proxy is being used.
