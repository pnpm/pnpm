## 1100.0.0

### Patch Changes

- On Windows, the `ERR_PNPM_BAD_ENV_FOUND` error of `pnpm setup` now shows the value `PNPM_HOME` is currently set to. It used to show the directory pnpm wanted to set instead.

- `@pnpm/config.env-replace`, `@pnpm/log.group`, `@pnpm/network.agent`, `@pnpm/network.ca-file`, `@pnpm/network.config`, `@pnpm/network.proxy-agent`, and the `@pnpm/os.env.path-extender` packages are now published from the pnpm repository. They are ES modules.

- `addDirToPosixEnvPath` now keeps the lines between two `# pnpm` sections of a shell config file when it overwrites the pnpm section [#12282](https://github.com/pnpm/pnpm/issues/12282).

- On Windows, `pnpm setup` repairs the `PNPM_HOME` registry type left by older pnpm versions, even when the configured directory has not changed.

- Updated dependencies:
  - @pnpm/error@1100.2.1
