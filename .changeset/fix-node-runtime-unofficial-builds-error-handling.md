---
"@pnpm/engine.runtime.node-resolver": patch
"pacquet": patch
"pnpm": patch
---

`pnpm update` no longer produces inconsistent lockfile entries for Node.js runtime dependencies when network requests to unofficial-builds.nodejs.org fail [pnpm/pnpm#14813](https://github.com/pnpm/pnpm/issues/14813).
