---
"@pnpm/store.cafs": patch
"@pnpm/worker": patch
"@pnpm/fetching.binary-fetcher": patch
"pnpm": patch
---

fix: preserve bundled `node_modules` from Node.js Windows zip so that npm/npx shims are created correctly on Windows.

The Windows Node.js distribution places npm inside a root-level `node_modules/` directory of the zip archive. `addFilesFromDir` was skipping root-level `node_modules` (to avoid treating a package's own npm dependencies as part of its content), which caused the bundled npm to be missing after installation. This prevented `pnpm env use` from creating the npm and npx shims on Windows.

Added an `includeNodeModules` option to `addFilesFromDir` and set it to `true` in the binary fetcher so that the complete Node.js distribution, including its bundled npm, is preserved.
