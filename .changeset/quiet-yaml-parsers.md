---
"@pnpm/assert-project": patch
"@pnpm/installing.commands": patch
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

pnpm now reads lockfiles with upstream js-yaml 4.3.2, which includes fixes for parsing merge keys and ordered maps [#18246](https://github.com/pnpm/pnpm/issues/18246).
