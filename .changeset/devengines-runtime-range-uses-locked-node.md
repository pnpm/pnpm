---
"pacquet": patch
---

`pnpm install` no longer skips optional dependencies that the Node.js version locked for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).
