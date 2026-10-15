## 1102.1.9

### Patch Changes

- pnpm now relinks the bins of a pnpm version it installed earlier for `packageManager` or `pnpm with`. A pnpm 12 version installed by a pnpm release older than 11.28.4 could keep a launcher that ran `node` on the native binary, so every command failed with `SyntaxError: Invalid or unexpected token` [#16646](https://github.com/pnpm/pnpm/issues/16646).

- `pnpm setup` no longer tries to install pnpm globally from npm's `bin` directory when pnpm was installed with npm. It failed with `Failed to create bin at <PNPM_HOME>/bin/pnpm` [#16855](https://github.com/pnpm/pnpm/issues/16855).

- Updated dependencies:
  - @pnpm/building.during-install@1102.2.8
  - @pnpm/building.policy@1100.1.7
  - @pnpm/config.reader@1102.3.5
  - @pnpm/config.version-policy@1100.2.8
  - @pnpm/global.commands@1102.0.8
  - @pnpm/installing.client@1100.3.16
  - @pnpm/installing.deps-restorer@1103.2.5
  - @pnpm/installing.env-installer@1103.0.11
  - @pnpm/resolving.npm-resolver@1104.2.6
  - @pnpm/store.connection-manager@1101.3.4
  - @pnpm/store.controller@1102.2.5
