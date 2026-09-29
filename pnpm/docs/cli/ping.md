---
id: ping
title: pnpm ping
---

Added in: v11.0.0

Ping the configured registry to verify connectivity.

```sh
pnpm ping [--registry <url>]
```

On success, the registry's response is printed. This is useful for quickly confirming that the current machine can reach the registry without installing or publishing anything.

## Options

### --registry &lt;url\>

The registry to ping. Defaults to the configured default registry.
