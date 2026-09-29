---
id: cache-path
title: pnpm cache path
---

Added in: v11.22.0

Prints the directory pnpm uses for its metadata cache, mirroring [`pnpm store path`](./store.md#path). The path is printed absolute and lexically cleaned, so a relative [`cacheDir`](../settings/other.md#cachedir) yields a path other tools can consume.

```sh
pnpm cache path
```

pnpm derives this directory from the platform and the `cacheDir` setting, so a CI setup that wants to cache it no longer has to mirror that resolution by hand:

```yaml title=".github/workflows/ci.yaml"
- name: Get the cache directory
  run: echo "PNPM_CACHE_DIR=$(pnpm cache path)" >> $GITHUB_ENV
```

Besides registry metadata and the [`pnpm dlx`](./pnx.md) cache, that directory holds the lockfile verification log — the record of which lockfile passed which [supply-chain policies](../supply-chain-security.md). Restoring it lets a job skip re-verifying a lockfile the log already covers under the policies currently configured — the dominant cost of an install in CI once the store is warm. Since v11.22.0, [`pnpm store prune`](./store.md#prune) no longer deletes that log.

:::important

Only cache this directory in locations writable by trusted jobs. See the [`cacheDir`](../settings/other.md#cachedir) setting for details.

:::
