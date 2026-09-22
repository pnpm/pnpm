---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now uses the running Node.js when `devEngines.runtime` declares a range without `onFail: download`. Optional dependencies supported by the active Node.js are no longer skipped [pnpm/pnpm#15230](https://github.com/pnpm/pnpm/issues/15230).
