---
"pacquet": patch
---

Dependency install scripts now find the node-gyp bundled with pnpm when pnpm runs through a symlink, such as `node_modules/.bin/pnpm` or the `pnpm` that `npm install -g pnpm` links. They used to fail with `node-gyp: command not found` on macOS [#15694](https://github.com/pnpm/pnpm/issues/15694).
