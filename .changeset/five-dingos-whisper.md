---
"pnpm": major
---

The `NODE_PATH` env variable is not set in the command shims (the files in `node_modules/.bin`). This env variable was really long and frequently caused errors on Windows.

Also, the `extend-node-path` setting is removed.

Related PR: [#4253](https://github.com/pnpm/pnpm/pull/4253)
