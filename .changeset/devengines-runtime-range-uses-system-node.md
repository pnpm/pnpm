---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

When `devEngines.runtime` declares a range without `onFail: download`, `pnpm install` now uses the Node.js already on the system. Previously, it compared each package's engines against the lower bound of the range and skipped supported optional dependencies. Only an entry that sets `onFail` to `download` provisions that lower bound [pnpm/pnpm#15230](https://github.com/pnpm/pnpm/issues/15230).
