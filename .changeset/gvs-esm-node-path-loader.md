---
"@pnpm/exec.esm-node-path-loader": minor
"@pnpm/config.reader": minor
"@pnpm/exec.commands": minor
"pnpm": minor
"pacquet": minor
---

When `enableGlobalVirtualStore` is on, every process pnpm spawns for the project (`pnpm run`, `pnpm exec`, lifecycle scripts) now receives a `NODE_PATH` pointing at the project's hoisted `node_modules`, plus a `NODE_OPTIONS` `--import` flag that registers a resolve hook restoring `NODE_PATH` lookups for ESM imports. Dependencies that import undeclared ("phantom") packages keep resolving under the global virtual store — for both CommonJS and ESM — without installing the `@pnpm/plugin-esm-node-path` config dependency [pnpm/pnpm#9618](https://github.com/pnpm/pnpm/issues/9618). Tools run by `pnpm dlx` resolve such dependencies too: the JS CLI passes them the same environment, while the Rust CLI's dlx cache is self-contained, so its layout already exposes them.
