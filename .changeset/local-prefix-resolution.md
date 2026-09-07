---
"pacquet": patch
---

Commands run from a subdirectory that has no `package.json` of its own now act on the nearest ancestor directory that has one, as they do on pnpm 11. `pnpm bin` printed a `node_modules/.bin` path under the current directory, which does not exist, so tools spawning executables out of it failed [#14622](https://github.com/pnpm/pnpm/issues/14622). `pnpm init` still creates its `package.json` in the current directory.
