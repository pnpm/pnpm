---
"pacquet": patch
---

The `runtime` setting is now read. Setting `runtime: false` in `pnpm-workspace.yaml`, `pnpm_config_runtime=false`, or `--config.runtime=false` stops `pnpm install`, `pnpm add`, `pnpm dedupe`, and the other installing commands from installing the runtimes in `devEngines.runtime` and `engines.runtime`. `pnpm dedupe` now also accepts `--no-runtime`.
