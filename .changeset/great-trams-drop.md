---
"@pnpm/plugin-commands-config": minor
"@pnpm/workspace.manifest-writer": minor
"@pnpm/workspace.read-manifest": minor
"@pnpm/constants": minor
"@pnpm/config": minor
"pnpm": minor
---

Add support for a global YAML config file named `config.yaml`.

Now configurations are divided into 2 categories:

- Registry and auth settings which can be stored in INI files such as global `rc` and local `.npmrc`.
- pnpm-specific settings which can only be loaded from YAML files such as global `config.yaml` and local `pnpm-workspace.yaml`.
