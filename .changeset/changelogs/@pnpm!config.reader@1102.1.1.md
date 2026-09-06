## 1102.1.1

### Patch Changes

- A relative `scriptShell` path in `pnpm-workspace.yaml` is now resolved against the workspace root, so scripts run from a nested workspace package find the shell [#14422](https://github.com/pnpm/pnpm/issues/14422). A bare command name such as `bash` is still looked up on `PATH`.

- `globalDir` and `globalBinDir` are honored wherever they are set, so `pnpm add -g` no longer fails with `ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH` after `pnpm config set -g global-bin-dir` [#14336](https://github.com/pnpm/pnpm/issues/14336). The global `config.yaml` is read again, `PNPM_CONFIG_GLOBAL_DIR` / `PNPM_CONFIG_GLOBAL_BIN_DIR` reach the directories derived from them, and a leading `~/` is expanded before that derivation. A project's `pnpm-workspace.yaml` still cannot set either key.

- Fixed `--side-effects-cache`/`--no-side-effects-cache` and `PNPM_CONFIG_SIDE_EFFECTS_CACHE` discarding a remote side-effects cache declared under the object form of `sideEffectsCache` in `pnpm-workspace.yaml`. The boolean now switches only the local cache off or on, as it already does when a config file declares it.

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.7
  - @pnpm/error@1100.1.4
  - @pnpm/hooks.pnpmfile@1100.0.30
  - @pnpm/pkg-manifest.utils@1100.4.3
  - @pnpm/workspace.project-manifest-reader@1100.0.27
  - @pnpm/workspace.workspace-manifest-reader@1100.1.9
