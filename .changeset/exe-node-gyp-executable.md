---
"@pnpm/exe": patch
"pnpm": patch
---

Restore the execute bit on the `node-gyp` shims packed inside `@pnpm/exe` (`dist/node-gyp-bin/node-gyp`, `dist/node-gyp-bin/node-gyp.cmd`, and `dist/node_modules/node-gyp/bin/node-gyp.js`). Without this, `pnpm/action-setup`'s standalone path (used on runners with Node.js < 22.13) failed any install whose lifecycle script invoked `node-gyp rebuild` with `sh: 1: node-gyp: Permission denied` [#11483](https://github.com/pnpm/pnpm/issues/11483).
