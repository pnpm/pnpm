## 1101.3.1

### Patch Changes

- `pnpm deploy` no longer installs the dependencies of the workspace root project into the deploy directory [#6437](https://github.com/pnpm/pnpm/issues/6437).

- `pnpm install --force` now reinstalls dependencies when the manifest and lockfile are unchanged. It previously reported "Already up to date" without reinstalling. Files changed in `node_modules` are restored when the store content is intact. Combining `--force` with `--frozen-store` now reports a configuration conflict on repeat installs [#919](https://github.com/pnpm/pnpm/issues/919).

- The `minimumReleaseAge` approval prompt now counts and displays each package version once [pnpm/pnpm#15083](https://github.com/pnpm/pnpm/issues/15083).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.5
  - @pnpm/building.policy@1100.1.2
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/config.reader@1102.2.1
  - @pnpm/config.writer@1100.0.27
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/deps.github-actions@1100.1.10
  - @pnpm/deps.inspection.outdated@1100.1.31
  - @pnpm/deps.path@1101.0.3
  - @pnpm/deps.security.signatures@1102.0.4
  - @pnpm/deps.status@1100.1.23
  - @pnpm/fs.graceful-fs@1100.2.2
  - @pnpm/global.commands@1102.0.2
  - @pnpm/global.packages@1101.1.3
  - @pnpm/hooks.pnpmfile@1100.0.32
  - @pnpm/installing.context@1101.0.5
  - @pnpm/installing.dedupe.check@1100.1.13
  - @pnpm/installing.deps-installer@1104.1.3
  - @pnpm/installing.env-installer@1103.0.5
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/lockfile.types@1100.1.2
  - @pnpm/lockfile.utils@1102.1.3
  - @pnpm/network.fetch@1100.1.17
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/resolving.npm-resolver@1104.2.0
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/store.connection-manager@1101.1.3
  - @pnpm/store.controller@1102.1.3
  - @pnpm/text.sanitize@1100.0.1
  - @pnpm/workspace.project-manifest-reader@1100.0.29
  - @pnpm/workspace.project-manifest-writer@1100.0.17
  - @pnpm/workspace.projects-filter@1100.0.43
  - @pnpm/workspace.projects-graph@1100.0.38
  - @pnpm/workspace.projects-reader@1101.0.28
  - @pnpm/workspace.root-finder@1100.0.9
  - @pnpm/workspace.state@1100.0.44
  - @pnpm/workspace.workspace-manifest-writer@1100.2.1
