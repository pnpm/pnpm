---
"@pnpm/resolving.parse-wanted-dependency": patch
---

`parseWantedDependency` now accepts `undefined` and treats it as an empty string. It previously threw a `TypeError`.
