---
"@pnpm/pnpr": minor
---

Renamed and regrouped pnpr configuration keys:

- An upstream registry's `maxAge` is now `packumentTtl`, the same name as the global setting it overrides.
- Byte limits take size strings such as `4MiB`. `oci.maxBlobBytes`, `oci.maxManifestBytes`, and `oci.chunkMaxBytes` are now `oci.maxBlobSize`, `oci.maxManifestSize`, and `oci.maxChunkSize`. `artifacts.quota.ownerGiB` and `artifacts.quota.totalGiB` are now `artifacts.quota.perOwner` and `artifacts.quota.total`.
- `secrets.cache` and `secrets.oci` are now `secret.cache` and `secret.oci`. `secret` takes either one string or that table.
- `trustedProxies` is now a table. The proxy list moves to `trustedProxies.networks` and `trustedProxyHeader` moves to `trustedProxies.header`.
- `log.type` is gone. pnpr always logs to stdout.
- New `listen` and `publicUrl` keys set the bind address and the public URL. `--listen` and `--public-url` override them.

pnpr now derives the private cache key from `secret`. Private cache entries written by an earlier version are no longer used, and `pnpr cache gc` removes them.

An unset environment variable in `database.url` now fails startup, as it already did in the other credential fields. An unset variable where an empty string is not a valid value, such as a number, also fails startup and names the variable.

An unparsable `routes.public` registry URL or package glob now fails startup. Before, pnpr logged a warning and ignored the rule.

pnpr logs a warning when it listens on a non-loopback address without `publicUrl`. `--storage` no longer replaces a `cache` path set in the config.
