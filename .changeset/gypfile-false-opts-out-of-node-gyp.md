---
"@pnpm/building.pkg-requires-build": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/store.cafs": patch
"@pnpm/types": patch
"pnpm": patch
"pacquet": patch
---

A dependency that ships a `binding.gyp` and sets `gypfile: false` no longer gets the `node-gyp rebuild` install script pnpm synthesizes for it. Such a dependency needs no `allowBuilds` entry and is no longer listed under "Ignored build scripts".
