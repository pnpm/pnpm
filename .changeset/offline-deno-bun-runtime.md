---
"@pnpm/engine.runtime.deno-resolver": patch
"@pnpm/engine.runtime.bun-resolver": patch
"pnpm": patch
---

The `deno` and `bun` runtime resolvers now fail fast with a clear error when resolution is requested in offline mode, instead of attempting to reach GitHub for release assets. This matches the existing behavior of the Node.js runtime resolver.
