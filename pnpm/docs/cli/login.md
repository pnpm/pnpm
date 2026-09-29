---
id: login
title: pnpm login
---

Added in: v11.0.0

Aliases: `adduser`

Authenticate with an npm registry.

```sh
pnpm login [--registry <url>] [--scope <scope>]
```

Supports web-based login with QR code as well as classic username/password authentication.

Since v11.19.0, web-based login no longer requires an interactive terminal: without a TTY, `pnpm login` prints the authentication URL (skipping the QR code and the prompt to open the URL in a browser) and polls the registry until the browser approval completes. Only the classic username/password login still fails with `ERR_PNPM_LOGIN_NON_INTERACTIVE` in a non-interactive terminal.

In pnpm 12.1 and newer, the granted token is written to the global
[`config.yaml`](./config.md), under the structured [`_auth`](../npmrc.md#_auth)
setting. When `--scope` is present, the same write routes that scope to the
registry under the global [`registries`](../registries.md) setting. pnpm 11.25
writes the token and scoped route to
[`<pnpm config>/auth.ini`](../npmrc.md#auth-file-locations); pnpm 12.1 continues
to read tokens written there.

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
