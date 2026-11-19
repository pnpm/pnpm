---
"@pnpm/exec.esm-node-path-loader": patch
"pnpm": patch
"pacquet": patch
---

With `enableGlobalVirtualStore` on, `require.extensions` hooks run again for `require()` calls from a module without a parent, such as the module Next.js uses to load `next.config.ts`. A required `.ts` file skipped the registered TypeScript hook. A required `.json` file was parsed as JavaScript [#18261](https://github.com/pnpm/pnpm/issues/18261).
