## 1101.4.6

### Patch Changes

- `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records for workspace projects missing from disk. This affected `pnpm remove` and installs that leave the lockfile unchanged, such as frozen installs. Before, a following frozen install failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).

- A filtered install, such as `pnpm install --filter <pkg>`, now keeps the `pnpm-lock.yaml` entries of workspace projects that are missing from disk, as in a sparse checkout. Their importers, packages, and catalog entries stay even when the filter matches every project on disk [#16931](https://github.com/pnpm/pnpm/issues/16931).

- With `sharedWorkspaceLockfile: false`, an injected workspace package installed in the same run as its dependent now holds only the files its `files` field selects. The copy used to also hold other files of the project, such as `tsconfig.json` [#16683](https://github.com/pnpm/pnpm/issues/16683).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.11
  - @pnpm/building.policy@1100.1.7
  - @pnpm/config.reader@1102.3.5
  - @pnpm/config.version-policy@1100.2.8
  - @pnpm/config.writer@1100.0.33
  - @pnpm/deps.github-actions@1100.1.16
  - @pnpm/deps.inspection.outdated@1100.1.37
  - @pnpm/deps.status@1100.1.30
  - @pnpm/global.commands@1102.0.8
  - @pnpm/installing.context@1101.0.11
  - @pnpm/installing.deps-installer@1104.2.5
  - @pnpm/installing.env-installer@1103.0.11
  - @pnpm/resolving.npm-resolver@1104.2.6
  - @pnpm/store.connection-manager@1101.3.4
  - @pnpm/store.controller@1102.2.5
  - @pnpm/workspace.injected-deps-syncer@1100.0.44
  - @pnpm/workspace.projects-filter@1100.0.49
  - @pnpm/workspace.projects-graph@1100.0.44
  - @pnpm/workspace.projects-reader@1101.1.4
  - @pnpm/workspace.root-finder@1100.1.3
  - @pnpm/workspace.state@1100.0.50
  - @pnpm/workspace.workspace-manifest-writer@1100.2.7
