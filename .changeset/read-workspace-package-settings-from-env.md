---
"pacquet": patch
---

pnpm now reads the `linkWorkspacePackages`, `injectWorkspacePackages`, `extendNodePath`, `frozenStore`, and `optimisticRepeatInstall` settings from `PNPM_CONFIG_*` environment variables, such as `PNPM_CONFIG_LINK_WORKSPACE_PACKAGES=true`. `--config.extend-node-path` now applies too. Before, pnpm ignored these environment variables.
