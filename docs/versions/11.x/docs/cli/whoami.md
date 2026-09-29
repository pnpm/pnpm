---
id: whoami
title: pnpm whoami
---

Added in: v11.0.0

Print the username associated with the current registry credentials.

```sh
pnpm whoami [--registry <url>]
```

If you are not logged in, the command exits with an error. Use [`pnpm login`](./login.md) to authenticate first.

## Options

### --registry &lt;url\>

The registry to check. Defaults to the configured default registry.
