---
"pacquet": patch
---

`pnpm add <pkg>` without a version now installs the version under the dist-tag set by the `tag` setting. An `npm:` alias or a `jsr:` dependency without a version resolves that tag too. The setting is read from `pnpm-workspace.yaml`, the global config, `PNPM_CONFIG_TAG`, and `--config.tag`, and defaults to `latest`.
