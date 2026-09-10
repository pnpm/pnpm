---
"pacquet": patch
---

Under `nodeLinker: hoisted`, an install no longer re-imports packages that are already in place. A package the previous install recorded at that directory in `.modules.yaml` (`hoistedLocations`), whose directory still holds a `package.json` of the recorded version, is left alone and is not handed to the build phase; pnpm 11 makes the same check. Before, every install replaced the whole tree through a staging directory, so a repeat `pnpm install` on a hoisted workspace took longer than a fresh one and reported `Packages: +N`. Still imported every time: a directory the user removed, one whose `package.json` no longer matches, a `file:` directory dependency, and a patched package. Still built: a package the previous install left ignored or pending, and every package when `pnpm rebuild` runs or `allowBuilds` changed.
