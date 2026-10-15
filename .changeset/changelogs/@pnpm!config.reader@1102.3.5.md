## 1102.3.5

### Patch Changes

- pnpm again accepts `httpProxy` and `httpsProxy` set to `null` or written with no value in `pnpm-workspace.yaml` and the global config file, and treats them as unset. Since 11.28.5 these values failed with `ERR_PNPM_INVALID_SETTING`.

- A key under a registry URL in the `_auth` setting that does not start with `@` is now skipped with a warning, so a field a later pnpm version adds there no longer breaks this one. A key that holds an `authToken` object but lacks its `@`, such as `org` for `@org`, is still an error.

- pnpm now switches to the pnpm version a project pins even when it cannot read its configuration. Previously, a setting it rejected in the global config file, an `.npmrc` file, `_auth`, or `pnpm-workspace.yaml` stopped every command, `pnpm --version` included.
