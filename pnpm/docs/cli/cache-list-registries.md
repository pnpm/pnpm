---
id: cache-list-registries
title: pnpm cache list-registries
---

:::warning

This command is experimental

:::

Lists all registries that have their metadata cache locally, one URL per line:

```
https://registry.npmjs.org/
https://npm.example.com:8443/team-a/
```

Since v12.4.0, the metadata cache is keyed by the registry's full URL, including its path and scheme, so two registries served from one host under different paths no longer share cached packuments and metadata fetched over HTTP is never reused for HTTPS. Up to v12.4.2 this command printed the cache directory name, such as `https%3A+registry.npmjs.org`, and since v12.5.0 it prints the URL, matching [`pnpm cache view`](./cache-view.md). A script that parsed the old host-shaped directory names, here or in `pnpm cache list`, needs updating, and the first install after upgrading refetches registry metadata. The package store is untouched.
