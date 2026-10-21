## 1102.3.6

### Patch Changes

- The warning about unrecognized settings in `pnpm-workspace.yaml` and the global `config.yaml` now names `addMissingPeerTypes`, `autoDedupe`, `bootConfig`, `macosBackup`, `nodePackageMap`, `permissions`, `pipelineBase`, `publishWaitTimeout`, `python`, `remoteCache`, `saveTypes`, `skills`, `storeIndexFormat`, and `tools` as pnpm v12 settings [pnpm/tasks#167](https://github.com/pnpm/tasks/issues/167).

- pnpm now records the pinned pnpm version in the `pnpm-lock.yaml` under the `lockfileDir` set in `pnpm-workspace.yaml`, and resolves a relative `lockfileDir` from the workspace root. pnpm used to write a second `pnpm-lock.yaml` at the workspace root.

- pnpm now recognizes `externalDependencies` and `autoInstallPeersFromHighestMatch` in `pnpm-workspace.yaml` without warning and forwards both settings to `pnpm install`. Previously, setting `externalDependencies` in `pnpm-workspace.yaml` with `nodeLinker: hoisted` crashed `pnpm install` with `externalDependencies.has is not a function` [pnpm/tasks#166](https://github.com/pnpm/tasks/issues/166).

- Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).

- `pmOnFail` and `lockfileDir` in `pnpm-workspace.yaml` now stay in force when another setting in that file cannot be read. pnpm used to switch to the pinned version despite `pmOnFail: ignore` in that case.

- pnpm now ignores a `PNPM_CONFIG_*` environment variable for a URL setting, such as `PNPM_CONFIG_REGISTRY`, when its value is not a URL. Such a value used to make every command fail with `Invalid URL`.

- Updated dependencies:
  - @pnpm/exec.esm-node-path-loader@1100.0.2
  - @pnpm/hooks.pnpmfile@1100.0.37
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
  - @pnpm/workspace.workspace-manifest-reader@1100.3.0
