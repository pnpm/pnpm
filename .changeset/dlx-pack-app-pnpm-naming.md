---
"pacquet": patch
---

`pnpm dlx` now keeps the virtual store of its cached installs in `node_modules/.pnpm` instead of `node_modules/.pacquet` [pnpm/pnpm#13955](https://github.com/pnpm/pnpm/issues/13955).

`pnpm pack-app` now names the manifest of its runtime install directory `pnpm-pack-app-<target>` instead of `pacquet-pack-app-<target>`.
