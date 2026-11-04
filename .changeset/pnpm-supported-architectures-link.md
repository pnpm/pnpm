---
"pacquet": minor
---

When `supportedArchitectures` lists exactly one platform and not `current`, pnpm links that platform's variant of a package published with platform variants, together with that variant's dependencies, to prepare `node_modules` for another machine. A floor or ABI that the listed platform does not state counts as not met, so the version's own archive is linked instead. Changing the setting relinks the package on the next install, even from a frozen lockfile.
