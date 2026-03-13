---
"@pnpm/config": patch
"pnpm": patch
---

Engine validation now uses the Node.js version from `devEngines.runtime` (or `engines.runtime`) instead of the system Node.js version [#10033](https://github.com/pnpm/pnpm/issues/10033).
