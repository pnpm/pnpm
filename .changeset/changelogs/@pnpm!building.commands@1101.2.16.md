## 1101.2.16

### Patch Changes

- `pnpm approve-builds` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal. `pnpm dlx` and global installs also skip automatic build-approval prompts in CI, leaving pending builds unapproved.

- `pnpm dlx` now keys its cache on active `overrides`, `packageExtensions`, `minimumReleaseAge`, `trustPolicy`, `trustLockfile`, and `blockExoticSubdeps` settings. Cached installs created under looser settings are no longer reused when stricter policies apply.

  `pnpm dlx --allow-build=!<pkg>` now denies build scripts for `<pkg>`, including the executed package itself. Denied packages also skip interactive build approval [#18075](https://github.com/pnpm/pnpm/pull/18075).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.17
  - @pnpm/building.policy@1100.1.11
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.reader@1102.3.10
  - @pnpm/config.writer@1100.0.38
  - @pnpm/deps.path@1101.0.10
  - @pnpm/global.packages@1101.1.12
  - @pnpm/installing.commands@1101.4.12
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/store.connection-manager@1101.3.10
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.projects-sorter@1101.0.5
  - @pnpm/workspace.task-scheduler@1100.0.8
  - @pnpm/workspace.workspace-manifest-reader@1100.3.3
