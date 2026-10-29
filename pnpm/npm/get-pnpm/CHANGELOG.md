# get-pnpm

## 0.0.11

### Patch Changes

- `npx get-pnpm --no-modify-path`, or `PNPM_NO_MODIFY_PATH=1`, installs pnpm without adding it to PATH. Your shell config file and the Windows registry are left unchanged, and `pnpm setup` prints the lines to add yourself. It needs pnpm 12.16.0 or newer. An older version is refused before anything changes [#17349](https://github.com/pnpm/pnpm/pull/17349).

## 0.0.10

### Patch Changes

- `npx get-pnpm` no longer sends an auth token from npm config to a registry served over plain HTTP, unless the registry is on the local machine.

- When Corepack runs pnpm and the native binary has to be downloaded, the download now uses the proxy from pnpm's config. That proxy comes from `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, the global `config.yaml`, and the user `.npmrc`. Previously, the download read the proxy only from `HTTPS_PROXY`, `HTTP_PROXY`, and `NO_PROXY` [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).
