## 1102.0.14

### Patch Changes

- `pnpm approve-builds` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal. `pnpm dlx` and global installs also skip automatic build-approval prompts in CI, leaving pending builds unapproved.

- `pnpm update -g --latest` now upgrades global runtimes (`node`, `deno`, and `bun`) and `npm:`-aliased global packages across major versions.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.43
  - @pnpm/bins.remover@1100.0.33
  - @pnpm/bins.resolver@1100.0.21
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.reader@1102.3.10
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/deps.inspection.list@1101.1.5
  - @pnpm/engine.runtime.node-resolver@1101.3.14
  - @pnpm/global.packages@1101.1.12
  - @pnpm/installing.deps-installer@1104.2.11
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/resolving.local-resolver@1101.2.10
  - @pnpm/store.connection-manager@1101.3.10
  - @pnpm/types@1102.1.4
