---
"@pnpm/building.pkg-requires-build": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/store.cafs": patch
"@pnpm/types": patch
"pnpm": patch
"pacquet": patch
---

pnpm now reads `gypfile: false` from a dependency's manifest as an opt-out of the `node-gyp rebuild` install script it synthesizes for a package that ships a `binding.gyp` and declares no `install` or `preinstall` script. Such a package no longer needs an `allowBuilds` entry and is no longer listed under "Ignored build scripts". npm has read the field this way for years. `better-sqlite3` v13 sets it and ships a prebuilt binary for every platform it supports.
