---
"@pnpm/global.commands": minor
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm update -g` no longer changes the pnpm version. pnpm's own global install belongs to `pnpm self-update`, and a global update could silently replace it with a different release [#14270](https://github.com/pnpm/pnpm/issues/14270).
