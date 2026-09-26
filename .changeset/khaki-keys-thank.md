---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
"pacquet": patch
---

A script that runs `pnpm run` no longer adds duplicate `node_modules/.bin` and `node-gyp-bin` entries to `PATH` [#5352](https://github.com/pnpm/pnpm/issues/5352).
