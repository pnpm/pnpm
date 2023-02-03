---
"@pnpm/plugin-commands-installation": patch
"@pnpm/hooks.read-package-hook": patch
"@pnpm/core": patch
"pnpm": patch
---

Use Map rather than Object in `createPackageExtender` to prevent read the prototype property to native function
