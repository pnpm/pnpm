# get-pnpm

## 0.1.0

### Minor Changes

- pnpm now provides a prebuilt binary for Linux on LoongArch (loong64) with glibc.

### Patch Changes

- When the `pnpm` package downloads its native binary on first run, it now reads the registry and credentials from the global `auth.ini`, where `pnpm login` writes them, and from the `_auth` setting of the global `config.yaml`.

- `npx get-pnpm` now reads a `pnpm-workspace.yaml` or `package.yaml` that holds only blank lines or comments as empty. It failed with "expected a document, but the input is empty".

- `npx get-pnpm` now installs pnpm from a registry that carries no npm signatures, such as Azure Artifacts, by checking the download against the signatures on registry.npmjs.org. It failed with "carries no npm registry signature".

- When the `pnpm` package downloads its native binary on first run, a `${NPM_TOKEN?}` placeholder in `.npmrc` now expands to the variable. It expanded to an empty value, so the download was sent without credentials.

- When the `pnpm` package downloads its native binary on first run, credentials configured for a registry path, such as `//registry.example.com/org-a/npm/:_authToken`, are now sent only to that path. They were sent to any path on the same host.

## 0.0.13

### Patch Changes

- When the `pnpm` package downloads its native binary on first run, it now reads a global `config.yaml` that holds only blank lines or comments as no settings. Such a file made every command fail with "expected a document, but the input is empty" [#18172](https://github.com/pnpm/pnpm/issues/18172).

- When the `pnpm` package downloads its native binary on first run, it now works with registries that serve no per-version package documents, such as Azure Artifacts. If such a registry also removed npm's signatures, pnpm fetches them from registry.npmjs.org. The binary still comes from the configured registry and is checked against npm's signature.

## 0.0.12

### Patch Changes

- pnpm now provides a prebuilt binary for NetBSD 10 on x64 [#17803](https://github.com/pnpm/pnpm/issues/17803).

- `get-pnpm` now installs the version that a project in a hidden directory pins when the workspace's `packages` patterns exclude that project. It used to install the workspace root's pin for that project.

## 0.0.11

### Patch Changes

- `npx get-pnpm --no-modify-path`, or `PNPM_NO_MODIFY_PATH=1`, installs pnpm without adding it to PATH. Your shell config file and the Windows registry are left unchanged, and `pnpm setup` prints the lines to add yourself. It needs pnpm 12.16.0 or newer. An older version is refused before anything changes [#17349](https://github.com/pnpm/pnpm/pull/17349).

## 0.0.10

### Patch Changes

- `npx get-pnpm` no longer sends an auth token from npm config to a registry served over plain HTTP, unless the registry is on the local machine.

- When Corepack runs pnpm and the native binary has to be downloaded, the download now uses the proxy from pnpm's config. That proxy comes from `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, the global `config.yaml`, and the user `.npmrc`. Previously, the download read the proxy only from `HTTPS_PROXY`, `HTTP_PROXY`, and `NO_PROXY` [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).
