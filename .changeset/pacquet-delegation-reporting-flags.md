---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install --silent` no longer fails when the install is delegated to pacquet. pnpm also stops passing `-s`, `--loglevel` and the other reporting flags to pacquet [#11936](https://github.com/pnpm/pnpm/issues/11936).
