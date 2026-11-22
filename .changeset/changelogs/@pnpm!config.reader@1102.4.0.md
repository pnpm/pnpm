## 1102.4.0

### Minor Changes

- `otp` is no longer read from any config file. Pass `--otp` on the command line, or set `PNPM_CONFIG_OTP`. A `pnpm-workspace.yaml` or a global `config.yaml` that carries `otp` is ignored, with a warning naming those two channels. `pnpm config set otp` fails with `ERR_PNPM_CONFIG_SET_NOT_A_FILE_SETTING` instead of writing an entry no loader reads back.

  This closes a way to leak a secret. A repository could write `otp: ${NPM_TOKEN}` beside its own `registry:`, and `pnpm publish` sent that environment variable to the registry the same file named, in the `npm-otp` header of its first request [#13542](https://github.com/pnpm/pnpm/issues/13542).

### Patch Changes

- Tab completion suggests project scripts when you type `pnpm` followed by a script name [#18239](https://github.com/pnpm/pnpm/issues/18239).

- `pnpm dlx`, `pnx`, and `pnpm create` now handle local isolation, script skipping, and non-registry package specifiers consistently:

  - `pnpm` ignores local `.pnpmfile.cjs` hooks and project `.npmrc` proxy or TLS settings in the working directory.
  - Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
  - Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
  - Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.

- Empty or whitespace-only numeric `pnpm_config_*` environment variables no longer override configured values or defaults.

- Global commands (`--global`, `-g`) no longer load `.pnpmfile.cjs`, `pnpm-workspace.yaml`, or `.npmrc` from the current working directory. `pnpm why -g` now inspects globally installed packages.

- Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

- A `tasks` entry with no value in `pnpm-workspace.yaml`, such as `build:`, now fails with a hint. The hint suggests writing `{}` for a task with no settings, or removing the entry to keep the default `dependsOn: ['^<task>']`.

- pnpm now reports `referenceTime` in `pnpm-workspace.yaml` as a setting of a newer pnpm version.

- Internal type-safety cleanup with no change in behavior.

- TLS file paths in `.npmrc` and `auth.ini` resolve relative to the file that declares them. Project `.npmrc` files skip TLS file paths containing environment variables.

  `pnpm config get` refuses to print protected auth settings. `pnpm config list` hides TLS private keys and inline proxy credentials. `pnpm config set --global tokenHelper` writes to `auth.ini`. Setting a project-level `tokenHelper` fails.

- A setting in `pnpm-workspace.yaml` or the global `config.yaml` that references an undefined environment variable without a fallback now fails with `ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR`. pnpm used to fail with an error that had no code [#10963](https://github.com/pnpm/pnpm/pull/10963).

- Reject unsupported types for the `noProxy` setting in workspace and global configuration. Strings and booleans remain valid, and `null` leaves the setting unset. Previously, unsupported values could reach proxy resolution without a validation error.

- Validate `sideEffectsCache.exclude` in workspace configuration and add it to the `SideEffectsCacheSettings` type definition.

- Validate `strictSsl` and `localAddress` settings in workspace configuration.

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/exec.esm-node-path-loader@1100.0.3
  - @pnpm/hooks.pnpmfile@1100.0.41
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
  - @pnpm/workspace.workspace-manifest-reader@1100.3.4
