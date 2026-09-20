## 1102.2.1

### Patch Changes

- Warnings about ignored environment variables in project `.npmrc` credentials now link to the npmrc documentation [pnpm/pnpm#15051](https://github.com/pnpm/pnpm/issues/15051).

- pnpm now reads a `pnpm-workspace.yaml` whose `tasks` section uses a setting only pnpm 12 acts on, such as `concurrencyGroup`. A task's unrecognized fields are ignored, unless the field only differs in case from `concurrency` or `dependsOn`, which pnpm reports as a typo.

  The warning about unrecognized top-level settings now names `cargo`, `concurrencyGroups`, and `pipelines` as pnpm 12 settings.

- Updated dependencies:
  - @pnpm/hooks.pnpmfile@1100.0.32
  - @pnpm/network.git-utils@1100.0.4
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/workspace.project-manifest-reader@1100.0.29
