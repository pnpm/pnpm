---
id: logout
title: pnpm logout
---

Log out of an npm registry. Revokes the authentication token on the registry and removes it from the local auth config file.

```sh
pnpm logout [--registry <url>]
```

Since v12.1.0, pnpm removes a token written by `pnpm login` from the global
[`config.yaml`](./config.md), and also checks
[`<pnpm config>/auth.ini`](../npmrc.md#auth-file-locations) for a token written
by an earlier version. A token supplied by `.npmrc`, another config file, or the
environment is revoked at the registry but must be removed from that source
manually.

## Options

### --registry &lt;url\>

The registry to log out from. Defaults to the configured default registry.
