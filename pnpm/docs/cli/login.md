---
id: login
title: pnpm login
---

Aliases: `adduser`

Authenticate with an npm registry.

```sh
pnpm login [--registry <url>] [--scope <scope>]
```

Supports web-based login with QR code as well as classic username/password authentication.

Web-based login does not require an interactive terminal. Without a TTY, `pnpm login` prints the authentication URL (skipping the QR code and the prompt to open the URL in a browser) and polls the registry until the browser approval completes. Only the classic username/password login still fails with `ERR_PNPM_LOGIN_NON_INTERACTIVE` in a non-interactive terminal.

Since v12.13.0, web-based login prints the registry's `npm-notice` messages before the authentication URL. Some registries use them to show a verification code that you confirm in the browser.

Since v12.1.0, the granted token is written to the global
[`config.yaml`](./config.md), under the structured [`_auth`](../npmrc.md#_auth)
setting. When `--scope` is present, the same write routes that scope to the
registry under the global [`registries`](../registries.md) setting. pnpm still
reads tokens written to [`<pnpm config>/auth.ini`](../npmrc.md#auth-file-locations).

## Options

### --registry &lt;url\>

The registry to authenticate with. Defaults to the configured default registry.

### --scope &lt;scope\>

Associate the credentials with the specified scope. The registry for that scope will be used.

Since v12.1.0, a `scope` setting in a project's
`pnpm-workspace.yaml` is ignored with a warning. A repository must not be able
to choose the machine-wide route a later login writes. Pass `--scope`, set
`PNPM_CONFIG_SCOPE`, or set the machine's default with:

```sh
pnpm config set --global scope @acme
```
