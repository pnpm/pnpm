---
"pacquet": patch
---

Under `nodeLinker: hoisted`, `pnpm install` no longer re-imports packages that are already in place. A repeat install used to replace the whole `node_modules` tree and report `Packages: +N`. A package is still imported when its directory is missing, when its `package.json` no longer carries the installed version, when it is a `file:` dependency, and when it is patched. Lifecycle scripts no longer run again for a package left in place, but `pnpm rebuild` and a change to `allowBuilds` still reach it.
