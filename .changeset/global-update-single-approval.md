---
"@pnpm/global.commands": patch
"pnpm": patch
---

`pnpm update -g` now asks once for approval of an immature version when `minimumReleaseAgeStrict` is enabled. The update resolved each global group twice and prompted for the same versions on both passes [#15091](https://github.com/pnpm/pnpm/issues/15091).
