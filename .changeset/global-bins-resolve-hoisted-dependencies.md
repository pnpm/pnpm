---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"@pnpm/global.commands": patch
"@pnpm/installing.context": patch
"pnpm": patch
"pacquet": patch
---

Commands installed with `pnpm add --global` now find the dependencies that pnpm hoists for their package. A package that loads a dependency it does not declare, such as a native addon's platform-specific package, failed with "Cannot find module". Reinstall such a package with `pnpm add --global` to update its existing command [#17503](https://github.com/pnpm/pnpm/issues/17503).
