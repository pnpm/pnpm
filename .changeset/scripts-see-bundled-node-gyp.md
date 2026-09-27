---
"pacquet": patch
---

`pnpm run` and lifecycle scripts now set `npm_config_node_gyp` to the bundled `node-gyp` entry point. Tools that read the variable resolve the same `node-gyp` pnpm builds with. An `npm_config_node_gyp` value the environment already sets is kept as is [#16270](https://github.com/pnpm/pnpm/issues/16270).
