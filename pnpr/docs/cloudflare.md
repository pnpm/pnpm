---
id: cloudflare
title: Deploying on Cloudflare
---

pnpr is a long-running server, so on Cloudflare it runs as a
[Container](https://developers.cloudflare.com/containers/) rather than as a
Worker. A small Worker receives every request and forwards it to one container
running the published pnpr image. The container sleeps when idle and starts
again on the next request, so an idle registry costs nothing beyond the plan
fee.

The state lives outside the container, where it survives sleeps and restarts:

- **Hosted packages** go to an R2 bucket through the
  [S3 backend](storage.md). R2 is S3-compatible and charges nothing for egress.
- **Users and tokens** go to a [Turso](https://turso.tech) database through the
  [libsql auth backend](auth-backends.md). Cloudflare's own databases do not
  speak a protocol pnpr can use.
- **The upstream cache** and the publish journal stay on the container's disk,
  which is wiped when the container stops. The cache refills on demand. The
  journal caveat for ephemeral disks in [Storage](storage.md#s3-publish-journal-and-lifecycle-caveats)
  applies.

## What you need

- A Cloudflare account on the Workers Paid plan. Containers are not on the free
  plan.
- [Wrangler](https://developers.cloudflare.com/workers/wrangler/) and Docker on
  the machine you deploy from. Wrangler builds the image locally and pushes it
  to Cloudflare's registry.
- An R2 bucket and an R2 API token with Object Read and Write on it. Note the
  access key ID and secret access key.
- A Turso database and a read-write token for it. Note the `libsql://` URL.

## The project

Four files make up the deployment. Put them in one directory.

The image wraps the published one and adds your config:

```dockerfile title="Dockerfile"
FROM ghcr.io/pnpm/pnpr:<version>
COPY pnpr.yaml /pnpr/pnpr.yaml
CMD ["serve", "--listen", "0.0.0.0:7677", "--config", "/pnpr/pnpr.yaml", "--cache", "/pnpr/cache"]
```

The config points the hosted store at the bucket and auth at Turso. The
credentials arrive as environment variables, which the Worker below passes
into the container:

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
    # No replicaPath: Turso Cloud does not serve the protocol the embedded
    # replica uses, so every token lookup is a remote query.

secret: ${PNPR_SECRET}

auth:
  htpasswd:
    file: /pnpr/cache/htpasswd
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
      "class_name": "PnprContainer",
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

The Worker forwards every request to one named container instance and hands
the secrets to it as environment variables. `PNPR_PUBLIC_URL` is read by pnpr
as `--public-url`, like every other flag's `PNPR_` variable:

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
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    return getContainer(env.PNPR, "pnpr").fetch(request);
  },
};
```

Install the two dependencies the Worker needs:

```sh
pnpm add -D wrangler
pnpm add @cloudflare/containers
```

## Deploy

Store the credentials as Worker secrets. Each command prompts for the value,
so nothing lands in a file or your shell history:

```sh
pnpm wrangler secret put AWS_ACCESS_KEY_ID
pnpm wrangler secret put AWS_SECRET_ACCESS_KEY
pnpm wrangler secret put PNPR_LIBSQL_URL
pnpm wrangler secret put PNPR_LIBSQL_TOKEN
openssl rand -hex 32 | pnpm wrangler secret put PNPR_SECRET
```

Then deploy. The first deploy prints the Worker's URL; put it in
`PNPR_PUBLIC_URL` and deploy once more:

```sh
pnpm wrangler deploy
```

The first request after a deploy or a sleep starts the container, which takes
a few seconds. Check it with the health endpoint, then point pnpm at the
registry's npm base:

```sh
curl https://pnpr.<your-subdomain>.workers.dev/-/ping
pnpm config set registry https://pnpr.<your-subdomain>.workers.dev/npm/
```

A change to the Worker alone, such as a new variable, deploys without
rebuilding the image:

```sh
pnpm wrangler deploy --containers-rollout=none
```

## What to expect

- **TLS and the hostname** come from Cloudflare, which is the
  [reverse proxy](tls-and-reverse-proxy.md) this setup runs behind. Add a
  custom domain to the Worker to serve the registry from your own hostname.
- **One instance** is enough for a team registry. The Worker above always
  forwards to the single container named `pnpr`, so raising `max_instances`
  alone starts nothing more. To run replicas, spread requests across several
  names or use the `getRandom` helper from the same package. Replicas share the
  bucket and the database, which the
  [S3 backend supports](storage.md#concurrent-writers), but each keeps its own
  upstream cache.
- **Publishing passes through the Worker**, so the plan's request body limit
  caps the size of a published tarball.
- **Container output** is in the Workers logs in the Cloudflare dashboard.
  `wrangler tail` shows the Worker's own events only.
- **Billing has no hard cap.** `max_instances` and `sleepAfter` bound what a
  forgotten deployment can cost, and a budget alert in the dashboard sends an
  email when projected spend crosses a threshold.
