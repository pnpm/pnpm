## 1100.1.0

### Minor Changes

- `pnpm config set` refuses to write a setting to a project's `pnpm-workspace.yaml` that pnpm does not read from there, rather than leaving a key in the file that does nothing. Those settings are `configDir`, `pnpmHomeDir`, `stateDir` and the others that name machine-level state. The command fails with `ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING`, naming where the setting does belong when it belongs somewhere. `pnpm config delete` still clears one that a file already carries, in whichever spelling it uses [#13629](https://github.com/pnpm/pnpm/issues/13629).

### Patch Changes

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.23
  - @pnpm/config.reader@1101.17.0
  - @pnpm/error@1100.1.2
  - @pnpm/object.property-path@1100.1.4
  - @pnpm/workspace.workspace-manifest-writer@1100.1.0
