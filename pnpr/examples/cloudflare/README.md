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
  const variant = metadataVariant(request.headers.get('accept'))
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
registry. Purge both metadata variants after package writes, using `metadataKey`
with `metadataVariant(null)` and
`metadataVariant('application/vnd.npm.install-v1+json')`.

Run the policy tests from the repository root:

```sh
node --test pnpr/examples/cloudflare/cache-policy.test.mjs
```
