---
"pacquet": patch
---

Commands run from a subdirectory of a project now act on the nearest ancestor directory that has a manifest, as they do on pnpm 11. `pnpm bin` printed a `node_modules/.bin` path under the current directory, which does not exist [#14622](https://github.com/pnpm/pnpm/issues/14622). `pnpm init` still creates its `package.json` in the current directory, and `pnpm exec` still runs its command there.
