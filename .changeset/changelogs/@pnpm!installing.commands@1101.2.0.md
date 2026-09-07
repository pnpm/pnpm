## 1101.2.0

### Minor Changes

- `pnpm remove` and `pnpm update` now accept `--trust-lockfile`, `--no-trust-lockfile`, `--trust-policy`, `--trust-policy-exclude` and `--trust-policy-ignore-after`, the same flags `pnpm install` and `pnpm add` take, so the supply-chain settings can be overridden for a single run. `pnpm remove` verifies the lockfile against the active policies the way `pnpm install` does, and `--trust-lockfile` skips that pass for every entry, not only the package being removed.

  The Rust CLI now also honors `--config.trust-lockfile=<value>`, and accepts the bare `--trust-lockfile` / `--no-trust-lockfile` spelling on the commands that previously took the setting from the config file alone.

### Patch Changes

- Build scripts can now be rejected before the package is installed [#14067](https://github.com/pnpm/pnpm/issues/14067):

  - `pnpm add --allow-build=!<pkg>` records `<pkg>: false` in `allowBuilds`. It used to write a `!<pkg>: true` entry that matched no package. On a global install the denial was dropped altogether, so the post-install prompt offered the build for approval.
  - `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` record their decision even when no packages are awaiting approval, and report a package that is not awaiting approval with a warning so a typo stays visible. Both cases used to fail the command with an error.

- `pnpm import` now leaves the project-local lockfile unchanged when `lockfileDir` points to another directory. Failed imports restore the destination lockfile. Imports that use a branch lockfile leave the shared lockfile unchanged [#14563](https://github.com/pnpm/pnpm/issues/14563).

- pnpm 12 now accepts the boolean settings as command-line flags on every command that takes them in pnpm 11, for example `pnpm install --unsafe-perm`, `pnpm add foo --offline`, and `pnpm install --dangerously-allow-all-builds`. pnpm 12 rejected them with `unexpected argument`, which failed every install on Vercel, whose build runs `pnpm install --unsafe-perm` [#14346](https://github.com/pnpm/pnpm/issues/14346).

  `pnpm remove` now accepts `--unsafe-perm`, the same flag `pnpm install`, `pnpm add`, and `pnpm update` take.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.3
  - @pnpm/building.policy@1100.1.0
  - @pnpm/catalogs.config@1100.0.7
  - @pnpm/cli.utils@1101.0.26
  - @pnpm/config.reader@1102.1.1
  - @pnpm/config.version-policy@1100.2.3
  - @pnpm/config.writer@1100.0.25
  - @pnpm/deps.github-actions@1100.1.8
  - @pnpm/deps.inspection.outdated@1100.1.29
  - @pnpm/deps.security.signatures@1102.0.2
  - @pnpm/deps.status@1100.1.21
  - @pnpm/error@1100.1.4
  - @pnpm/global.commands@1102.0.0
  - @pnpm/global.packages@1101.1.1
  - @pnpm/hooks.pnpmfile@1100.0.30
  - @pnpm/installing.context@1101.0.3
  - @pnpm/installing.dedupe.check@1100.1.12
  - @pnpm/installing.deps-installer@1104.1.1
  - @pnpm/installing.env-installer@1103.0.3
  - @pnpm/lockfile.fs@1100.2.6
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/lockfile.utils@1102.1.1
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/network.fetch@1100.1.15
  - @pnpm/pkg-manifest.reader@1100.0.19
  - @pnpm/pkg-manifest.utils@1100.4.3
  - @pnpm/resolving.npm-resolver@1104.1.1
  - @pnpm/store.connection-manager@1101.1.1
  - @pnpm/store.controller@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.0.27
  - @pnpm/workspace.projects-filter@1100.0.41
  - @pnpm/workspace.projects-graph@1100.0.36
  - @pnpm/workspace.projects-reader@1101.0.26
  - @pnpm/workspace.root-finder@1100.0.8
  - @pnpm/workspace.state@1100.0.42
  - @pnpm/workspace.task-scheduler@1100.0.1
  - @pnpm/workspace.workspace-manifest-reader@1100.1.9
  - @pnpm/workspace.workspace-manifest-writer@1100.1.3
