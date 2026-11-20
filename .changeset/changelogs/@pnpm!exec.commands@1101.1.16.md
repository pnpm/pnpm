## 1101.1.16

### Patch Changes

- `pnpm approve-builds` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal. `pnpm dlx` and global installs also skip automatic build-approval prompts in CI, leaving pending builds unapproved.

- Combining `--filter` and `--filter-prod` with exclusion selectors such as `!pkg` now evaluates all selectors together in command-line order. An exclusion passed on one flag while inclusions were passed on the other no longer selects every workspace project.

- `pnpm dlx` now keys its cache on active `overrides`, `packageExtensions`, `minimumReleaseAge`, `trustPolicy`, `trustLockfile`, and `blockExoticSubdeps` settings. Cached installs created under looser settings are no longer reused when stricter policies apply.

  `pnpm dlx --allow-build=!<pkg>` now denies build scripts for `<pkg>`, including the executed package itself. Denied packages also skip interactive build approval [#18075](https://github.com/pnpm/pnpm/pull/18075).

- `pnpm --if-present start`, `pnpm run --if-present start`, and `pnpm restart --if-present` now exit with code 0 when a project has neither a `start` script nor a `server.js` file [#18073](https://github.com/pnpm/pnpm/pull/18073).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.21
  - @pnpm/building.commands@1101.2.16
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.reader@1102.3.10
  - @pnpm/config.version-policy@1100.2.11
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/deps.status@1100.1.36
  - @pnpm/engine.runtime.commands@1101.1.15
  - @pnpm/engine.runtime.system-version@1100.0.16
  - @pnpm/exec.lifecycle@1100.1.30
  - @pnpm/exec.npm-lifecycle@1100.0.9
  - @pnpm/exec.pnpm-cli-runner@1100.0.8
  - @pnpm/installing.client@1100.3.22
  - @pnpm/installing.commands@1101.4.12
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.injected-deps-syncer@1100.0.50
  - @pnpm/workspace.project-manifest-reader@1100.1.7
  - @pnpm/workspace.projects-filter@1100.0.55
  - @pnpm/workspace.projects-reader@1101.1.8
  - @pnpm/workspace.projects-sorter@1101.0.5
  - @pnpm/workspace.task-scheduler@1100.0.8
