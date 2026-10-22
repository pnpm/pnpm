---
"pacquet": patch
---

`pnpm install` now reads the `runtime` setting. Setting `runtime: false` in `pnpm-workspace.yaml`, `pnpm_config_runtime=false` in the environment, or `--config.runtime=false` skips installing the Node.js, Bun, and Deno runtimes from `devEngines.runtime` and `engines.runtime`, as `--no-runtime` does.
