---
"@pnpm/global.commands": patch
"pnpm": patch
---

`pnpm update -g` no longer asks more than once for approval of the same immature `name@version` when `minimumReleaseAgeStrict` is enabled [#15091](https://github.com/pnpm/pnpm/issues/15091).
