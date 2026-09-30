---
"@pnpm/exec.esm-node-path-loader": patch
"pnpm": patch
"pacquet": patch
---

With `enableGlobalVirtualStore` on, scripts can run entry points that a CommonJS require hook loads again, such as `ts-node index.ts`. They failed with `ERR_UNKNOWN_FILE_EXTENSION` on Node.js versions without built-in TypeScript support [#16436](https://github.com/pnpm/pnpm/issues/16436).
