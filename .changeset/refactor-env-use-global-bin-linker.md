---
"@pnpm/constants": patch
"@pnpm/resolving.resolver-base": patch
"@pnpm/lockfile.types": patch
"@pnpm/engine.runtime.node.resolver": patch
"@pnpm/engine.runtime.commands": patch
---

Added `getNodeBinsForCurrentOS` to `@pnpm/constants` which returns a `Record<string, string>` with paths for `node`, `npm`, and `npx` within the Node.js package. This record is now used as `BinaryResolution.bin` (type widened from `string` to `string | Record<string, string>`) and as `manifest.bin` in the node resolver, so pnpm's bin-linker creates all three shims automatically when installing a Node.js runtime.
