---
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

The root project's `preinstall` script now runs before any dependency is installed, so it can no longer import dependencies. It ran after the dependencies were resolved and linked, so a guard such as `npx only-allow yarn` could not stop the install [#3760](https://github.com/pnpm/pnpm/issues/3760).
