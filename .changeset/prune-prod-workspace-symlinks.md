---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm prune` is now recursive by default inside a workspace, just like `pnpm install`. This fixes `pnpm prune --prod` in a workspace root emptying the `node_modules` directories of the other workspace projects, dropping the links to the workspace packages they depend on in production [#13718](https://github.com/pnpm/pnpm/issues/13718).
