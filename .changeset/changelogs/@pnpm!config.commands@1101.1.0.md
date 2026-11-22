## 1101.1.0

### Minor Changes

- `otp` is no longer read from any config file. Pass `--otp` on the command line, or set `PNPM_CONFIG_OTP`. A `pnpm-workspace.yaml` or a global `config.yaml` that carries `otp` is ignored, with a warning naming those two channels. `pnpm config set otp` fails with `ERR_PNPM_CONFIG_SET_NOT_A_FILE_SETTING` instead of writing an entry no loader reads back.

  This closes a way to leak a secret. A repository could write `otp: ${NPM_TOKEN}` beside its own `registry:`, and `pnpm publish` sent that environment variable to the registry the same file named, in the `npm-otp` header of its first request [#13542](https://github.com/pnpm/pnpm/issues/13542).

### Patch Changes

- The help for `pnpm config get` and `pnpm get` now says that the key is a property path, such as `catalog.react`.

- TLS file paths in `.npmrc` and `auth.ini` resolve relative to the file that declares them. Project `.npmrc` files skip TLS file paths containing environment variables.

  `pnpm config get` refuses to print protected auth settings. `pnpm config list` hides TLS private keys and inline proxy credentials. `pnpm config set --global tokenHelper` writes to `auth.ini`. Setting a project-level `tokenHelper` fails.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/config.reader@1102.4.0
  - @pnpm/error@1100.2.3
  - @pnpm/object.property-path@1100.1.10
  - @pnpm/workspace.workspace-manifest-writer@1100.2.13
