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

The granted token is written to
[`<pnpm config>/auth.ini`](../npmrc.md#auth-file-locations). Since v12.1.0,
`--scope` also routes that scope to the registry under the global
[`registries`](../registries.md) setting in [`config.yaml`](./config.md).

From v12.1.0 to v12.15.0, the token was written to the
[`_auth`](../npmrc.md#_auth) setting in the global `config.yaml`. There, an
unscoped token also made its registry the default. Since v12.16.0 a login
removes the token it replaces from `_auth`.

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
