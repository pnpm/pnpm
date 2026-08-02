---
"pacquet": patch
---

pnpm now ships `node-gyp` again, so packages whose install scripts shell out to it build out of the box. Previously they failed with `spawn node-gyp ENOENT` unless a `node-gyp` was already on `PATH` — affecting `node-gyp-build` with no matching prebuild, `node-pre-gyp`, a plain `"install": "node-gyp rebuild"`, and any package shipping a `binding.gyp` without an install script. As in pnpm 11, the whole `node-gyp` dependency tree is resolved from pnpm's own lockfile when pnpm is released, so it is frozen per release rather than resolved on your machine, and `npm_config_node_gyp`, a workspace `node-gyp`, and a package's own `node-gyp` dependency all still take precedence.
