---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm update <pkg>@<version>` now updates only the selected packages and leaves unrelated dependencies unchanged.
