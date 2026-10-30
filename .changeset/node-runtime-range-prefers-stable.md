---
"@pnpm/engine.runtime.node-resolver": patch
"pnpm": patch
"pacquet": patch
---

A Node.js version range such as `>=26.13`, for example in `devEngines.runtime`, now resolves to the newest stable release that satisfies it. pnpm picks a prerelease only when no stable release matches [pnpm/pnpm#17516](https://github.com/pnpm/pnpm/issues/17516).
