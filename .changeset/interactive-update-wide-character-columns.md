---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update --interactive` now measures its table in terminal columns rather than in characters. A package name, workspace name, or version containing wide characters (CJK, most emoji) no longer knocks its row's columns out of line with the rest of the group, and a wide character in a version no longer aborts the command with `Subject parameter value width cannot be greater than the container width` [#13357](https://github.com/pnpm/pnpm/issues/13357).
