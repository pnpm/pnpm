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
- Users and tokens go to a [Turso](https://turso.tech) database through the
  [libsql auth backend](auth-backends.md). Cloudflare's own databases do not
  speak a protocol pnpr can use, so Turso is the simplest option.
- The upstream cache and the publish journal stay on the container's disk.
  Losing the cache only means fetching from npm again. Losing the journal in
  the middle of a publish is the ephemeral-disk case described in
  [Storage](storage.md#s3-publish-journal-and-lifecycle-caveats).

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
storage: /pnpr/storage

s3:
  bucket: my-pnpr-packages
  region: auto
  endpoint: https://<account-id>.r2.cloudflarestorage.com
  # Credentials come from AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.

backend:
  libsql:
    url: ${PNPR_LIBSQL_URL}
    authToken: ${PNPR_LIBSQL_TOKEN}
    # Token lookups read a local replica of the Turso database. The replica
    # lives on the container's ephemeral disk and is rebuilt on each start.
    replicaPath: /pnpr/cache/auth-replica.db

secret: ${PNPR_SECRET}

# The Worker below passes the client's address in X-Real-IP.
trustedProxies: [10.0.0.0/8]
trustedProxyHeader: x-real-ip

auth:
  htpasswd:
    max_users: 3

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
    if (!this.ctx.container?.running) {
      try {
        await this.startAndWaitForPorts(7677, {
          instanceGetTimeoutMS: 120_000,
          portReadyTimeoutMS: 180_000,
          abort: request.signal,
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        return new Response(`pnpr is starting, retry shortly (${message})\n`, { status: 503 });
      }
    }
    return super.fetch(request);
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const headers = new Headers(request.headers);
    headers.set("accept-encoding", "zstd, gzip");
    headers.delete("x-forwarded-for");
    headers.set("x-real-ip", request.headers.get("cf-connecting-ip") ?? "");
    const response = await getContainer(env.PNPR, "pnpr").fetch(
      new Request(request, { headers }),
    );
    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers: response.headers,
      encodeBody: "manual",
    });
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

**The Worker caches nothing.** If you add a cache, store only `GET`
responses without `private`, `no-store`, or `no-cache` in `Cache-Control`.
A login answers with a new token and is marked only `no-cache`. Responses on
a named base such as `/npm/~<name>/` are always `private, no-store`, so only
the default base is cacheable. With [CORS](configuration.md#cors) on, pnpr's answer
depends on the request's `Origin`, so keep requests with an `Origin` header
out of a cache that does not key on it. A cache also outlives
[rule changes](configuration.md#changing-rules-through-the-api).

**Publishes go through the Worker**, so your plan's request body limit caps
the size of a package you can publish.

**Container logs are in the dashboard**, under the Worker's logs.
`wrangler tail` shows only the Worker's own events.

**There is no spending cap.** `max_instances` and `sleepAfter` bound what a
forgotten deployment can cost. A budget alert in the dashboard emails you when
projected spend crosses a threshold, but it does not stop anything.
