---
id: cache-list-registries
title: pnpm cache list-registries
---

:::warning

This command is experimental

:::

Lists all registries that have their metadata cache locally, one URL per line:

```text
https://registry.npmjs.org/
https://npm.example.com:8443/team-a/
```

The metadata cache is keyed by the registry's full URL, including its path and scheme. Two registries served from one host under different paths do not share cached metadata, and metadata fetched over HTTP is never reused for HTTPS.
