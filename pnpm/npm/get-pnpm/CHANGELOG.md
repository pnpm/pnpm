# get-pnpm

## 0.0.10

### Patch Changes

- When Corepack runs pnpm and the native binary has to be downloaded, the download now uses the proxy from pnpm's config. That proxy comes from `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, the global `config.yaml`, and the user `.npmrc`. Previously, the download read the proxy only from `HTTPS_PROXY`, `HTTP_PROXY`, and `NO_PROXY` [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).

- `npx get-pnpm` no longer sends an auth token from npm config to a registry served over plain HTTP, unless the registry is on the local machine.
