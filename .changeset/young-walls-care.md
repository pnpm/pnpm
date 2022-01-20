---
"@pnpm/plugin-commands-publishing": major
"pnpm": major
---

`pnpm pack` should only pack a file as an executable if it's a bin or listed in the `publishConfig.executableFiles` array.
