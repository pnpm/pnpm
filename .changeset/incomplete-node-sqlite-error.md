---
"@pnpm/store.index": patch
"pnpm": patch
---

pnpm now fails with `ERR_PNPM_INCOMPLETE_NODE_SQLITE` when the runtime's `node:sqlite` module lacks the methods the store index needs. The error names the missing methods. Before, runtimes with a partial `node:sqlite`, such as StackBlitz WebContainers, failed with "this.db.exec is not a function" [#15649](https://github.com/pnpm/pnpm/issues/15649).
