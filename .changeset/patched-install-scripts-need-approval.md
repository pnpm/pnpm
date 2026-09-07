---
"@pnpm/building.during-install": patch
"@pnpm/building.pkg-requires-build": patch
"pnpm": patch
"pacquet": patch
---

A patch that gives a dependency a `preinstall`, `install`, or `postinstall` script, or a `binding.gyp`, now runs that build. pnpm asks for build approval first, so the package is listed under "Ignored build scripts" until it is allowed to build. pnpm 12 ran nothing, and pnpm 11 ran it without asking [#14648](https://github.com/pnpm/pnpm/issues/14648).
