---
"@pnpm/constants": patch
"@pnpm/resolver-base": patch
"@pnpm/lockfile.types": patch
"@pnpm/node.resolver": patch
"@pnpm/plugin-commands-env": patch
---

Added `getNodeBinsForCurrentOS` to `@pnpm/constants` which returns a `Record<string, string>` with paths for `node`, `npm`, and `npx` within the Node.js package. This record is now used as `BinaryResolution.bin` (type widened from `string` to `string | Record<string, string>`) and as `manifest.bin` in the node resolver, so pnpm's bin-linker creates all three shims automatically when installing a Node.js runtime.
