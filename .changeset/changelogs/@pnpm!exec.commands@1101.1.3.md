## 1101.1.3

### Patch Changes

- `pnpm exec <command>` and `pnpm <command>` run from a subdirectory of a project now find the executables installed in the project's `node_modules/.bin`. The command still runs in the subdirectory. `PNPM_PACKAGE_NAME` names the project [#5068](https://github.com/pnpm/pnpm/issues/5068).

- `pnpm exec` and `pnpm dlx` now wait for the command to finish shutting down after `Ctrl+C`. A signal sent to pnpm alone now reaches the command, the way it does with `pnpm run`. pnpm used to exit on the interrupt and terminate the command while it was still shutting down [#7374](https://github.com/pnpm/pnpm/issues/7374).

- `pnpm dlx` and `pnx` now prompt to approve dependency build scripts in interactive terminals. Cached packages with pending builds also prompt for approval. Without an interactive terminal, use `--allow-build` to allow the required builds. Fixes [pnpm/pnpm#14943](https://github.com/pnpm/pnpm/issues/14943).

- Updated dependencies:
  - @pnpm/building.commands@1101.2.3
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/config.reader@1102.2.1
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/crypto.hash@1100.0.5
  - @pnpm/deps.status@1100.1.23
  - @pnpm/engine.runtime.commands@1101.1.3
  - @pnpm/exec.lifecycle@1100.1.18
  - @pnpm/exec.pnpm-cli-runner@1100.0.3
  - @pnpm/installing.client@1100.3.10
  - @pnpm/installing.commands@1101.3.1
  - @pnpm/workspace.injected-deps-syncer@1100.0.38
  - @pnpm/workspace.project-manifest-reader@1100.0.29
