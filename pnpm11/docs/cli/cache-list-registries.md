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

Since v11.27.1, the command prints each registry's URL. Earlier versions printed the cache directory name, which v11.27.0 changed to encode the registry's scheme and path, as in `https%3A+registry.npmjs.org`. A script that parsed those names, here or in `pnpm cache list`, needs updating.
