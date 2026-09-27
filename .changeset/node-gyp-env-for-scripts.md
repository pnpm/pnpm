---
"pacquet": patch
---

Scripts run by pnpm now receive `npm_config_node_gyp` pointing at the node-gyp copy bundled with pnpm, matching the behavior of the TypeScript CLI. A `npm_config_node_gyp` value already present in the environment, or one set through `extraEnv`, still takes precedence.
