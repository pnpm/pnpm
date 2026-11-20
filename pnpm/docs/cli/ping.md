---
id: ping
title: pnpm ping
---

Ping the configured registry to verify connectivity.

```sh
pnpm ping [--registry <url>]
```

On success, pnpm prints the registry URL and the round-trip time:

```text
PING https://registry.npmjs.org/
PONG 120ms
```

If the registry answers with a non-empty JSON object or array, pnpm prints it on a further `PONG` line. If the request fails, the command fails with `ERR_PNPM_PING_ERROR`.

This is useful for quickly confirming that the current machine can reach the registry without installing or publishing anything.

## Options

### --registry &lt;url\>

The registry to ping. Defaults to the configured default registry.
