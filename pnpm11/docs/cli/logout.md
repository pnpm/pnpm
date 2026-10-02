---
id: logout
title: pnpm logout
---

Added in: v11.0.0

Log out of an npm registry. Revokes the authentication token on the registry and removes it from the local auth config file.

```sh
pnpm logout [--registry <url>] [--scope <scope>]
```

If a scope is provided, the registry associated with that scope is used.

The token is removed from [`<pnpm config>/auth.ini`](../npmrc.md#auth-file-locations). A token supplied by `.npmrc`, another config file, or the environment is revoked at the registry but must be removed from that source manually.

## Options

### --registry &lt;url\>

The registry to log out from. Defaults to the configured default registry.

### --scope &lt;scope\>

Use the registry associated with the given scope.
