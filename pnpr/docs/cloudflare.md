---
id: cloudflare
title: Deploying on Cloudflare
---

You can run pnpr on Cloudflare, but not as a Worker. pnpr is an ordinary
long-running server, so it needs a
[Container](https://developers.cloudflare.com/containers/). The setup is one
container running the published pnpr image, plus a few lines of Worker that
pass every request through to it. When nobody has used the registry for a
while, the container goes to sleep, and the next request wakes it up. An idle
registry costs you nothing beyond the plan fee.

The container's disk is wiped whenever it stops, so everything that has to
survive lives somewhere else:

- Published packages go to an R2 bucket. pnpr talks to it through its
  [S3 backend](storage.md), and R2 charges nothing for egress.
- Package metadata, users, and tokens go to a [Turso](https://turso.tech)
  database, through [libSQL](storage.md#libsql-database) and the
  [libsql auth backend](auth-backends.md). Cloudflare's own databases do not
  speak a protocol pnpr can use, so Turso is the simplest option.
- The upstream cache stays on the container's disk. Losing it only means
  fetching from npm again. A container that stops in the middle of a publish
  leaves blobs no package references, as described in
  [Storage](storage.md#interruptions).

## Before you start

You need:

- A Cloudflare account on the Workers Paid plan. Containers are not available
  on the free plan.
- [Wrangler](https://developers.cloudflare.com/workers/wrangler/) and Docker on
  the machine you deploy from. Wrangler builds the image locally and pushes it
  to Cloudflare's registry.
- An R2 bucket, and an R2 API token with Object Read and Write on it. Keep the
  access key ID and the secret access key.
- A Turso database and a read-write token for it. Keep the `libsql://` URL.

## The project

The whole deployment is four files in one directory.

The Dockerfile takes the published image and adds your config:

```dockerfile title="Dockerfile"
FROM ghcr.io/pnpm/pnpr:<version>
COPY pnpr.yaml /pnpr/pnpr.yaml
CMD ["serve", "--listen", "0.0.0.0:7677", "--config", "/pnpr/pnpr.yaml", "--cache", "/pnpr/cache"]
```

The pnpr config points the hosted store at your bucket and auth at Turso. The
credentials arrive as environment variables, which the Worker below sets:

```yaml title="pnpr.yaml"
storage:
  path: /pnpr/storage
  s3:
    bucket: my-pnpr-packages
    region: auto
    endpoint: https://<account-id>.r2.cloudflarestorage.com
    # Credentials come from AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.
  database:
    url: ${PNPR_LIBSQL_URL}
    authToken: ${PNPR_LIBSQL_TOKEN}
    # Package views are read from a local replica, rebuilt on each start.
    replicaPath: /pnpr/cache/metadata-replica.db
    syncInterval: 10s

secret: ${PNPR_SECRET}

# The Worker below passes the client's address in X-Real-IP.
trustedProxies:
  networks: [10.0.0.0/8]
  header: x-real-ip

auth:
  users:
    maxUsers: 3
```

The registries live in the database, not in the image. Describe them in a file
that stays outside the image:

```yaml title="registries.yaml"
registries:
  local:
    type: hosted
    packages:
      '@mycompany/*':
        publish: $authenticated

  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true

  main:
    type: router
    sources: [local, npmjs]

defaultRegistry: main
```

Store them once from any machine that can reach the database, with the same
config and environment variables, using
[`pnpr registries import`](cli.md#registries). Leave `replicaPath` out of that
config unless its directory exists on that machine:

```sh
pnpr -c ./pnpr.yaml registries import ./registries.yaml
```

Without this step, the first start stores the
[default registries](configuration.md#registries-in-the-database).

The Wrangler config declares the container, the Durable Object that owns it,
and the public URL pnpr writes into tarball links:

```jsonc title="wrangler.jsonc"
{
  "name": "pnpr",
  "main": "src/index.ts",
  "compatibility_date": "2026-09-01",
  "workers_dev": true,
  "vars": { "PNPR_PUBLIC_URL": "https://pnpr.<your-subdomain>.workers.dev" },
  "containers": [
    {
      "name": "pnpr-container",
      "image": "./Dockerfile",
      "max_instances": 1,
      "instance_type": "standard-1"
    }
  ],
  "durable_objects": {
    "bindings": [{ "name": "PNPR", "class_name": "PnprContainer" }]
  },
  "exports": {
    "PnprContainer": {
      "type": "durable-object",
      "storage": "sqlite",
      "container": "pnpr-container"
    }
  }
}
```

The Worker forwards every request to the container and hands it the secrets.
One part of it deserves an explanation. When the container has been asleep,
Cloudflare can take 20 to 60 seconds to bring it back. The container library
waits only 20 seconds for the port by default and then fails the request with
a 500. The `fetch` override below waits longer, so the first request after a
sleep gets a slow answer instead of an error:

```ts title="src/index.ts"
import { Container, getContainer } from "@cloudflare/containers";

interface Env {
  PNPR: DurableObjectNamespace<PnprContainer>;
  AWS_ACCESS_KEY_ID: string;
  AWS_SECRET_ACCESS_KEY: string;
  PNPR_LIBSQL_URL: string;
  PNPR_LIBSQL_TOKEN: string;
  PNPR_SECRET: string;
  PNPR_PUBLIC_URL: string;
}

export class PnprContainer extends Container<Env> {
  defaultPort = 7677;
  sleepAfter = "10m";

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.envVars = {
      AWS_ACCESS_KEY_ID: env.AWS_ACCESS_KEY_ID,
      AWS_SECRET_ACCESS_KEY: env.AWS_SECRET_ACCESS_KEY,
      PNPR_LIBSQL_URL: env.PNPR_LIBSQL_URL,
      PNPR_LIBSQL_TOKEN: env.PNPR_LIBSQL_TOKEN,
      PNPR_SECRET: env.PNPR_SECRET,
      PNPR_PUBLIC_URL: env.PNPR_PUBLIC_URL,
    };
  }

  // Wait longer than the library's default for a cold start, so the first
  // request after a sleep gets an answer instead of a 500.
  override async fetch(request: Request): Promise<Response> {
    try {
      if (!this.ctx.container?.running) {
        await this.startAndWaitForPorts(7677, {
          instanceGetTimeoutMS: 120_000,
          portReadyTimeoutMS: 180_000,
          abort: request.signal,
        });
      }
      return await super.fetch(request);
    } catch (error) {
      return unavailable(error);
    }
  }
}

function unavailable(error: unknown): Response {
  console.error("Registry request failed", error);
  return new Response("Registry temporarily unavailable, retry shortly\n", {
    status: 503,
    headers: { "Retry-After": "5" },
  });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    try {
      decodeURIComponent(new URL(request.url).pathname);
    } catch (error) {
      if (!(error instanceof URIError)) throw error;
      return new Response("Malformed percent-encoded path\n", { status: 400 });
    }
    const headers = new Headers(request.headers);
    headers.set("accept-encoding", "zstd, gzip");
    headers.delete("x-forwarded-for");
    headers.set("x-real-ip", request.headers.get("cf-connecting-ip") ?? "");
    try {
      const response = await getContainer(env.PNPR, "pnpr").fetch(
        new Request(request, { headers }),
      );
      return new Response(response.body, {
        status: response.status,
        statusText: response.statusText,
        headers: response.headers,
        encodeBody: "manual",
      });
    } catch (error) {
      return unavailable(error);
    }
  },
};
```

The default export also fixes compression. Cloudflare removes `zstd` from a
request's `Accept-Encoding` before the Worker sees it, so without help pnpr
would never send zstd. The Worker asks pnpr for `zstd, gzip`, which is what
pnpm sends anyway, and `encodeBody: "manual"` hands the coded body back
untouched. Cloudflare then passes it as-is to clients that accept zstd and
decodes it for those that do not. This only matters for a
[tar registry](configuration.md#tar-registries); a `.tgz` is served as stored
whatever the request asks for.

It also passes on the client's address. pnpr sees every request come from
the container network, so without help a token limited to an address range
would check the wrong address, and login rate limiting would count all clients
as one. Cloudflare sets `CF-Connecting-IP` and does not let clients send it.
The Worker copies it into `X-Real-IP`, and the
[`trustedProxies`](tls-and-reverse-proxy.md#trustedproxies) setting in
`pnpr.yaml` makes pnpr read it.

`PNPR_PUBLIC_URL` works because pnpr reads every flag from an environment
variable with the `PNPR_` prefix. This one is `--public-url`.

Malformed percent-encoded paths receive HTTP `400` before the request reaches
the container.

Finally, install the two packages the Worker needs:

```sh
pnpm add -D wrangler
pnpm add @cloudflare/containers
```

## Deploying

Store the credentials as Worker secrets. Each command asks for the value, so
nothing ends up in a file or in your shell history:

```sh
pnpm wrangler secret put AWS_ACCESS_KEY_ID
pnpm wrangler secret put AWS_SECRET_ACCESS_KEY
pnpm wrangler secret put PNPR_LIBSQL_URL
pnpm wrangler secret put PNPR_LIBSQL_TOKEN
openssl rand -hex 32 | pnpm wrangler secret put PNPR_SECRET
```

Now deploy:

```sh
pnpm wrangler deploy
```

The first deploy prints the Worker's URL. Put it into `PNPR_PUBLIC_URL` in
`wrangler.jsonc` and deploy again.

Make a first request to wake the container, then point pnpm at the registry's
npm base:

```sh
curl https://pnpr.<your-subdomain>.workers.dev/-/ping
pnpm config set registry https://pnpr.<your-subdomain>.workers.dev/npm/
```

When you change only the Worker, for example to set a variable, you can skip
the image build:

```sh
pnpm wrangler deploy --containers-rollout=none
```

## Things to know

**The first request after a sleep is slow.** Expect 20 to 60 seconds. pnpr
itself starts in well under a second; the wait is Cloudflare bringing the
container back. If that bothers you, raise `sleepAfter`. The container then
stays up longer, and you pay for its idle memory and disk.

**TLS and the hostname come from Cloudflare.** This setup runs behind
Cloudflare as its [reverse proxy](tls-and-reverse-proxy.md). To serve the
registry from your own hostname, add a custom domain to the Worker.

**Tars reach other clients uncompressed.** Because the Worker always asks for
zstd, a client that accepts only gzip gets a tar that Cloudflare has decoded
back to plain bytes. Only pnpm installs from a tar registry, and it accepts
zstd, so in practice this affects someone fetching a tar by hand. Metadata is
not affected: Cloudflare compresses JSON again for gzip clients.

**One container is enough for a team.** The Worker above always talks to the
one container named `pnpr`, so raising `max_instances` on its own changes
nothing. If you do want replicas, spread requests over several container
names, or use the `getRandom` helper from the same package. Replicas can share
the bucket and the database, which the
[S3 backend supports](storage.md#concurrent-writers), but each one has its own
upstream cache.

The Worker above caches nothing. For caching, see
[CDN cache with push invalidation](#cdn-cache-with-push-invalidation). If you add a cache, store only `GET`
responses without `private`, `no-store`, or `no-cache` in `Cache-Control`.
A login answers with a new token and is marked only `no-cache`. With
[CORS](configuration.md#cors) on, pnpr's answer depends on the request's
`Origin`, so keep requests with an `Origin` header out of a cache that does
not key on it. A cache also outlives
[rule changes](configuration.md#changing-rules-through-the-api) and other
[changes to the stored registries](configuration.md#registries-in-the-database).

Metadata at one URL has several representations, selected by `Accept`. Keep
full JSON and `application/vnd.npm.install-v1+json` in separate cache entries.
Bypass caching when `Accept` offers `application/vnd.pnpr.install-v2+json`,
including when it also offers install-v1. Before storing metadata, check that
its response `Content-Type` matches the entry's representation. `Vary: Accept`
on the response does not partition a cache that builds its own keys.

Bypass both cache reads and writes for requests with a query string, including
`?write=true`, or request `Cache-Control: no-cache` or `no-store`. Such reads
must reach the registry. Bypass conditional requests too, so the registry can
evaluate their validators. Put representation selectors in the cache key's path
or query, never its fragment. When replacing a cache that mixed representations,
use a new key namespace or purge its entries.

For cache hits that do not contact the container, use the
[CDN cache example](#cdn-cache-with-push-invalidation). It caches public npm
metadata and package tarballs. Hosted package changes purge the package tag; access-rule
changes purge the deployment tag. Access revocation is eventually consistent.

The [revalidating metadata cache example](https://github.com/pnpm/pnpm/tree/main/pnpr/examples/cloudflare)
remains available when every read must check access and freshness at the registry.

**Publishes go through the Worker**, so your plan's request body limit caps
the size of a package you can publish. It also caps each OCI blob upload
request. A layer larger than that limit fails with HTTP `413` when the client
sends it in one `PATCH` or `PUT`, even if it fits `oci.maxBlobSize`.

For clients that honor the proposed `OCI-Chunk-Max-Length` header, configure
[the upload hint](configuration.md#oci) below your plan's request body limit:

```yaml title="pnpr.yaml"
oci:
  maxChunkSize: 50MiB
```

This hint cannot force a client to split requests. Docker and Podman uploads
may still send a whole layer in one request. For those clients, host pnpr
outside Cloudflare Containers behind your own
[TLS reverse proxy](tls-and-reverse-proxy.md), with a request body limit that
accommodates the largest layer. The Container in this guide is
[only reachable through its Worker](https://developers.cloudflare.com/containers/concepts/architecture/).
Adding a custom domain to that Worker does not bypass its limit. Raising
`oci.maxBlobSize` alone cannot raise Cloudflare's limit.

**Container logs are in the dashboard**, under the Worker's logs.
`wrangler tail` shows only the Worker's own events.

**There is no spending cap.** `max_instances` and `sleepAfter` bound what a
forgotten deployment can cost. A budget alert in the dashboard emails you when
projected spend crosses a threshold, but it does not stop anything.

## CDN cache with push invalidation

Use this option when a short delay after a publish, unpublish, or access change
is acceptable. Anonymous cache hits do not wake or contact the container.
Authenticated reads always reach pnpr. Previously downloaded public packages
cannot be recalled from clients or third-party caches.

Use a custom domain in a Cloudflare zone you control. Create an API token with
Cache Purge permission for that zone. The token belongs to the container, not
to clients. Add this to `pnpr.yaml`:

```yaml
cloudflareCdn:
  zoneId: ${CLOUDFLARE_ZONE_ID}
  apiToken: ${CLOUDFLARE_PURGE_TOKEN}
  cacheTag: example-registry
```

Choose a `cacheTag` unique to this registry deployment within the zone. All
servers sharing a database must use the same zone and tag. The database records
that destination when invalidation is first enabled. Changing the zone or tag
for an existing database is not supported. A mismatch is logged and prevents
purge delivery from that server; cached entries still expire. Set
`CLOUDFLARE_ZONE_ID` as a Worker variable and store `CLOUDFLARE_PURGE_TOKEN` as a
Worker secret. Add both to the Worker's `Env` interface and to the container's
`envVars`, following the other credentials in the example above.

Copy `cache-policy.mjs` and `cdn-cache.mjs` from
[the example directory](https://github.com/pnpm/pnpm/tree/main/pnpr/examples/cloudflare)
into `src/`. Import the cache helper:

```ts
import { fetchWithCdnCache } from "./cdn-cache.mjs";
```

Add `ctx: ExecutionContext` as the third argument of the default export's
`fetch` method. Replace its call to the container with:

```ts
const response = await fetchWithCdnCache(new Request(request, { headers }), {
  cache: caches.default,
  cacheTag: "example-registry",
  fetchFromRegistry: (forwarded: Request) => getContainer(env.PNPR, "pnpr").fetch(forwarded),
  waitUntil: (task: Promise<unknown>) => ctx.waitUntil(task),
});
```

The tag must match `cloudflareCdn.cacheTag`. Keep the malformed-path check,
forwarded client address, compression headers, and `encodeBody: "manual"`
response wrapper from the original Worker.

pnpr records package invalidations in the same database transaction as the
change. A background task submits pending tags to Cloudflare every five seconds.
A failed delivery remains pending across restarts. Package tags cover all
registry aliases, full and abbreviated metadata, and package tarball URLs.
Policy edits purge the deployment tag. Startup also queues that tag, covering
configuration changes. Writes through another process sharing the database
continue to record invalidations once this option has been enabled.

The example caches only successful responses explicitly marked public. Queries,
client validators, credentials, cookies, range requests, CORS requests, and
install-v2 metadata bypass it. Content-addressed tarball URLs also bypass it
because their paths do not identify a package to invalidate. Private responses
never populate the cache. Upstream changes have no push notification and become
visible through normal expiry and pnpr's upstream refresh policy.

Each cache entry expires five minutes after its origin request began. The
Worker never serves it stale, including during an origin outage. A response
that arrives after a purge can still fill the cache, but its original expiry
bounds that race. The same expiry bounds failed or delayed purges. Replica
synchronization and upstream refresh delays remain additional sources of
staleness. Responses served by this helper tell downstream caches not to store
another copy.

Cloudflare's purge API clears tags globally. `caches.default.delete()` only
clears the data center processing that call. See
[Cloudflare's Cache API purge documentation](https://developers.cloudflare.com/workers/reference/how-the-cache-works/#purge-assets-stored-with-the-cache-api).
Remove the old Worker's metadata and tarball cache lookup paths when switching
to this example, and purge any separate CDN cache rules. This helper uses a new
cache-key namespace; it does not read the old entries.

## Non-browser clients blocked with error 1010

Cloudflare's [Browser Integrity Check](https://developers.cloudflare.com/waf/tools/browser-integrity-check/)
can reject the default Python `urllib` User-Agent with HTTP `403` and error
`1010` before the request reaches your Worker. Changing headers inside the
Worker cannot fix this.

On a custom domain in your Cloudflare zone, create a WAF custom rule with a
Skip action for Browser Integrity Check. Limit it to the registry hostname
and paths, for example:

```text
http.host eq "registry.example.com" and
(starts_with(http.request.uri.path, "/npm/") or http.request.uri.path eq "/npm")
```

Include other API prefixes your clients use. Skip only Browser Integrity
Check, keeping authentication, rate limits, and other WAF protections active.
Check Security Events if requests still fail, and adjust the specific blocking
rule for those paths.

On `workers.dev`, [Browser Integrity Check cannot be disabled](https://developers.cloudflare.com/support/troubleshooting/http-status-codes/cloudflare-1xxx-errors/error-1010/).
Use a custom domain for clients that need their default User-Agent to work.
Scripts using the trial hostname can send a descriptive User-Agent:

```python
import urllib.request

request = urllib.request.Request(
    "https://pnpr.<your-subdomain>.workers.dev/npm/example-package",
    headers={"User-Agent": "registry-check/1.0"},
)
with urllib.request.urlopen(request) as response:
    print(response.status)
```

Verify your deployment with both the script and a malformed path:

```sh
curl --path-as-is -i 'https://registry.example.com/npm/%E0%A4%A'
```

The malformed path should return HTTP `400`. On a custom domain with the
exception, repeat the Python request without the `headers` argument to verify
that the default User-Agent also reaches the registry.
