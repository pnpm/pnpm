---
"@pnpm/resolving.parse-wanted-dependency": patch
"pnpm": patch
---

`parseWantedDependency` defaults to an empty string when called without arguments or with undefined.
