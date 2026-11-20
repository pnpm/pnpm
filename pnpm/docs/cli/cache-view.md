---
id: cache-view
title: pnpm cache view
---

:::warning

This command is experimental

:::

Views information from the specified package's cache.

```sh
pnpm cache view <pkg>
```

The output is a JSON object keyed by registry URL. Each entry lists `cachedVersions` (versions present in the store), `nonCachedVersions`, `cachedAt`, and `distTags`. When the cache directory does not exist, `{}` is printed.

```sh
pnpm cache view react
```
