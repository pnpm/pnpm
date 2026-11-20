---
id: logout
title: pnpm logout
---

Log out of an npm registry. Revokes the authentication token on the registry and removes it from the local auth config file.

```sh
pnpm logout [--registry <url>]
```

`pnpm logout` removes the token from
[`<pnpm config>/auth.ini`](../npmrc.md#auth-file-locations) and from the
[`_auth`](../npmrc.md#_auth) setting in the global [`config.yaml`](./config.md),
where pnpm 12.1 to 12.15 wrote it. A token supplied by `.npmrc`, another config file, or the
environment is revoked at the registry but must be removed from that source
manually.

## Options

### --registry &lt;url\>

The registry to log out from. Defaults to the configured default registry.
