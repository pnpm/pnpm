---
"@pnpm/config": patch
"pnpm": patch
---

`engineStrict` now uses the Node.js version from `devEngines.runtime` (or `engines.runtime`) when `onFail` is set to `"download"`, instead of using the system Node.js version [#10033](https://github.com/pnpm/pnpm/issues/10033).
