# Cloudflare caches

For cache hits that avoid the origin, use `cdn-cache.mjs` with pnpr's
`cloudflareCdn` configuration. The [setup and consistency contract](../../docs/cloudflare.md#cdn-cache-with-push-invalidation)
cover credentials, tag matching, retries, and the five-minute cache lifetime.
This helper caches anonymous public metadata and package tarballs. Hosted package
changes purge the package tag; policy changes purge the deployment tag.

The [deployment guide](../../docs/cloudflare.md) uses a pass-through Worker.
For metadata revalidation on every read, copy `cache-policy.mjs` and
`revalidate-metadata.mjs` alongside its entry point. They have no runtime
dependencies.

Call `fetchMetadata` only for package metadata routes. `base` and `name` come
from your registry route parser. Forward all other routes, including tarballs
and mutations, directly to the container:

```js
import { fetchMetadata } from './revalidate-metadata.mjs'

async function handleMetadata (request, ctx, route) {
  return fetchMetadata(request, {
    cache: caches.default,
    base: route.base,
    name: route.name,
    fetchFromRegistry,
    waitUntil: task => ctx.waitUntil(task),
  })
}
```

`fetchFromRegistry` must forward the supplied request, including its headers,
to the container. Set the registry request headers before calling `fetchMetadata`.
For example, set the deployment Worker's fixed `Accept-Encoding: zstd, gzip`
before selecting the cache key. Forward the original URL, never the synthetic cache key.

The `waitUntil` callback keeps cache updates alive after the response returns.
Cache API failures are logged and leave registry reads available.
Unchanged `304` responses reuse the stored body without rewriting it.

Every metadata read reaches the registry. A stored body supplies its ETag in
`If-None-Match`. A `304` reuses that body; a `200` replaces it. Access denials,
missing packages, and server errors reach the client without a stale fallback.
Requests retain their credentials during revalidation. Responses with cookies
or caller-dependent cache headers cannot populate the cache.

This trades an origin round trip per read for freshness without global purges.
A completed write does not need to enumerate metadata aliases or clear every
data center. Reads that overlap a write may still see the earlier state, but a
late cache fill must pass revalidation before a later read can use it. Freshness
and access checks remain the registry's responsibility, including any replica
synchronization delay.

The cache retains bodies for up to a day to reuse after a `304`; that retention
is never permission to serve them without revalidation. Responses preserve the
registry's cache headers, so downstream clients still follow those lifetimes.
Client conditional requests bypass cache storage and reach the registry with
their validators unchanged. This preserves its `304` handling, including weak
ETags and lists of validators.

Tarballs always pass through, including revision URLs such as
`widget@1.0.0+r1` and `/-/tarballs/sha512/` URLs. Their bytes may be immutable,
but access can change after an unpublish, a rule change, or a vulnerability
check. Use this pass-through option when every read must check access.

When replacing a Worker that cached tarballs or served fresh and stale metadata
without revalidation, remove its cache lookup paths. Purge any separate CDN
cache rules too. A local
[Cache API deletion](https://developers.cloudflare.com/workers/runtime-apis/cache/#delete)
clears only the data center handling that request. Changing this example's key
namespace cannot invalidate another cache in front of it.

The key separates full JSON, install-v1, and supported content encodings.
Install-v2, unsupported encoding preferences, queries, range requests,
conditional requests, CORS requests, and explicit cache bypasses go directly to
the registry. Cached compressed bytes retain their original content coding and
ETag when restored. Use `encodeBody: 'manual'` when wrapping the returned
response in a Worker so the bytes are not encoded twice.

Run the tests from the repository root:

```sh
node --test pnpr/examples/cloudflare/*.test.mjs
```
