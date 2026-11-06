# Cloudflare cache policy

This policy supports a custom caching Worker. The
[deployment guide](../../docs/cloudflare.md) uses a pass-through Worker.

Copy `cache-policy.mjs` alongside your Worker's entry point. It has no runtime
dependencies. It checks request bypasses, metadata representations, cache keys,
and response eligibility. It does not implement expiration, invalidation,
compression, or routing.

Use the checks before reading or writing metadata. `origin`, `base`, and `name`
below come from your registry route parser. Forward the original request to the
container, including its query and headers. Conditional requests bypass the
cache and let the registry evaluate their validators:

```js
import {
  bypassesCache, isShareable, matchesMetadata, metadataKey, metadataVariant,
} from './cache-policy.mjs'

async function fetchMetadata (request, origin, base, name, fetchFromRegistry) {
  const variant = metadataVariant(request.headers)
  if (bypassesCache(request) || !variant) return fetchFromRegistry(request)

  const key = new Request(metadataKey(origin, base, name, variant))
  const cached = await caches.default.match(key)
  if (cached) return cached

  const response = await fetchFromRegistry(request)
  if (response.status === 200 && isShareable(response) && matchesMetadata(response, variant)) {
    await caches.default.put(key, response.clone())
  }
  return response
}
```

The response must provide the cache lifetime. Preserve its cache headers when
storing it. The cache key is synthetic; send only the original URL to the
registry.

After package writes, invalidate both metadata representations and all cached
encodings on every cacheable route that resolves to the changed package. Include the default
`/npm/` route when a write arrives through a named base such as `/npm/~local/`.
Deleting only the named-base keys leaves the default-route entries cached.
Use `metadataKeys(origin, base, name)` to enumerate every key for each route.
[Cache API deletion](https://developers.cloudflare.com/workers/runtime-apis/cache/#delete)
affects only the current data center. Other locations can serve old metadata
until expiry unless you purge globally. An in-flight read can also store old
metadata after a purge. Coordinate cache fills with invalidation if you need
immediate consistency after writes.

Build the variant from the headers sent to the registry. The key includes
the metadata format and the request's `Accept-Encoding` value. Responses may
vary by `Accept` and `Accept-Encoding`. Other response selectors bypass storage because the cache key does not include them.

Caching supports an absent or empty `Accept-Encoding`, a single `identity`, `gzip`,
`deflate`, `br`, or `zstd` value, and the deployment Worker's fixed `zstd, gzip`
value. Other preferences bypass caching. This keeps the key set bounded and
lets `metadataKeys` enumerate it for invalidation.

Run the policy tests from the repository root:

```sh
node --test pnpr/examples/cloudflare/cache-policy.test.mjs
```
