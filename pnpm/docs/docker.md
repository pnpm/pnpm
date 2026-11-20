---
id: docker
title: Working with Docker
---

:::note

It is impossible to create reflinks or hardlinks between a Docker container and the host filesystem during build time.
The next best thing you can do is using BuildKit cache mount to share cache between builds. Alternatively, you may use
[podman] because it can mount Btrfs volumes during build time.
If you use BuildKit cache mounts, keep the pnpm store cache scoped to mutually trusted builds. A store cache that can be written by an untrusted build should not be reused by trusted builds.

:::

[podman]: ./podman.md

## Official pnpm base image

An official pnpm base image is published to GitHub Container Registry as [`ghcr.io/pnpm/pnpm`](https://github.com/pnpm/pnpm/pkgs/container/pnpm). It is based on `debian:stable-slim` and contains the pnpm [standalone binary], the `node-gyp` it bundles, and the few system libraries pnpm needs. Node.js is **not** bundled. This lets you pick the Node.js version yourself (inside your Dockerfile or at runtime) instead of being locked to whatever Node version a base image ships with.

[standalone binary]: ./installation.md#using-a-standalone-script

### Tags

| Tag                   | Meaning                                                                 |
| --------------------- | ----------------------------------------------------------------------- |
| `<version>`           | Exact, immutable (e.g. `12.0.0`). Includes prereleases.                 |
| `<major>`             | Tracks the latest stable release within that major (e.g. `12`).         |
| `latest`              | Highest stable pnpm version. Not updated for prereleases.               |
| `<version>-alpine`, `<major>-alpine`, `alpine` | The same, based on Alpine Linux. |

Supported platforms: `linux/amd64`, `linux/arm64`.

### Adding pnpm to your own image

Copy `/opt/pnpm` from the image whose C library matches your base image, and link the binary and its `pn`, `pnpx`, and `pnx` aliases onto `PATH`.
Copy the whole directory, because it also contains the `node-gyp` that pnpm uses to build native dependencies.

```dockerfile title="Dockerfile"
FROM node:24-alpine
COPY --from=ghcr.io/pnpm/pnpm:12-alpine /opt/pnpm /opt/pnpm
RUN for bin in pnpm pn pnpx pnx; do ln -s "/opt/pnpm/$bin" "/usr/local/bin/$bin"; done
```

For a Debian- or Ubuntu-based image, such as `node:24-slim`, copy from `ghcr.io/pnpm/pnpm:12` instead.

### Installing Node.js

Let pnpm install Node.js automatically from [`devEngines.runtime`](./package_json.md#devenginesruntime) in your `package.json`:

```json title="package.json"
{
  "devEngines": {
    "runtime": {
      "name": "node",
      "version": "22.x",
      "onFail": "download"
    }
  }
}
```

`pnpm install` downloads the runtime that `devEngines.runtime` declares, so the image needs no `pnpm runtime set`.

This example assumes the project has a `pnpm-workspace.yaml`. If it does not,
use `COPY package.json pnpm-lock.yaml ./` for the first copy step.

```dockerfile title="Dockerfile"
FROM ghcr.io/pnpm/pnpm:12
WORKDIR /app
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
CMD ["pnpm", "start"]
```

If your project has a `pnpm-workspace.yaml`, copy it before running
`pnpm install`. It contains project settings and, in a workspace, the package
patterns. Omitting it can change how dependencies are installed or make a frozen
install fail. Omit it from the `COPY` instruction only if your project has no
such file.

### When to use this image

- You want the Node.js version to be pinned by your project via `devEngines.runtime` rather than by the base image.
- You want to upgrade pnpm and Node.js independently.
- You prefer a minimal Debian base without the Node.js build toolchain.

The recipes further down this page start from this image and let pnpm install Node.js. If you prefer your own Node.js base image, keep the rest of each recipe and [install pnpm](./installation.md) into that image instead.

## Minimizing Docker image size and build time

* Use a small image, e.g. `ghcr.io/pnpm/pnpm` or `node:XX-slim`.
* Use multi-stage if possible and makes sense.
* Use BuildKit cache mounts.

The recipes below use the official pnpm image, which already sets `PNPM_HOME=/pnpm` and puts `/pnpm/bin` on `PATH`, so the default store is at `/pnpm/store`.

:::caution

Baking a warm store into an image layer (running an install during the build so later containers have nothing to download) does not make linking free. A hardlink to a file in a lower image layer succeeds, but overlayfs copies that file into the container's writable layer first, so an install copies most of the store out of the image while reporting that it hardlinked. [`packageImportMethod: copy`](./settings/node-modules.md#a-store-baked-into-a-container-image-layer) is usually faster in that setup, and measurably so on ext4. A BuildKit cache mount does not have this problem, because the store is a mount rather than a layer.

:::

When the base image has a runtime installed with `pnpm runtime set node 24 -g`,
keep the build cache at a separate location, such as `/var/cache/pnpm`. Mounting
a cache over `/pnpm/store` hides the managed runtime for the duration of the
`RUN` instruction. The examples below set `--store-dir` to the cache mount.

### Example 1: Build a bundle in a Docker container

Since `devDependencies` is only necessary for building the bundle, `pnpm install --prod` will be a separate stage
from `pnpm install` and `pnpm run build`, allowing the final stage to copy only necessary files from the earlier
stages, minimizing the size of the final image.

```text title=".dockerignore"
node_modules
.git
.gitignore
*.md
dist
```

```dockerfile title="Dockerfile"
FROM ghcr.io/pnpm/pnpm:12 AS base
RUN pnpm runtime set node 24 -g
COPY . /app
WORKDIR /app

FROM base AS prod-deps
RUN --mount=type=cache,id=pnpm,target=/var/cache/pnpm \
    pnpm install --store-dir /var/cache/pnpm --prod --frozen-lockfile

FROM base AS build
RUN --mount=type=cache,id=pnpm,target=/var/cache/pnpm \
    pnpm install --store-dir /var/cache/pnpm --frozen-lockfile
RUN pnpm run build

FROM base
COPY --from=prod-deps /app/node_modules /app/node_modules
COPY --from=build /app/dist /app/dist
EXPOSE 8000
CMD [ "pnpm", "start" ]
```

### Example 2: Build multiple Docker images in a monorepo

Assuming you have a monorepo with 3 packages: app1, app2, and common; app1 and app2 depend on common but not each other.

You want to save only necessary dependencies for each package, `pnpm deploy` should help you with copying only necessary files and packages.

```text title="Structure of the monorepo"
./
├── Dockerfile
├── .dockerignore
├── .gitignore
├── packages/
│   ├── app1/
│   │   ├── dist/
│   │   ├── package.json
│   │   ├── src/
│   │   └── tsconfig.json
│   ├── app2/
│   │   ├── dist/
│   │   ├── package.json
│   │   ├── src/
│   │   └── tsconfig.json
│   └── common/
│       ├── dist/
│       ├── package.json
│       ├── src/
│       └── tsconfig.json
├── pnpm-lock.yaml
├── pnpm-workspace.yaml
└── tsconfig.json
```

```yaml title="pnpm-workspace.yaml"
packages:
  - 'packages/*'
syncInjectedDepsAfterScripts:
- build
injectWorkspacePackages: true
```

```text title=".dockerignore"
node_modules
.git
.gitignore
*.md
dist
```

```dockerfile title="Dockerfile"
FROM ghcr.io/pnpm/pnpm:12 AS base
RUN pnpm runtime set node 24 -g

FROM base AS build
COPY . /usr/src/app
WORKDIR /usr/src/app
RUN --mount=type=cache,id=pnpm,target=/var/cache/pnpm \
    pnpm install --store-dir /var/cache/pnpm --frozen-lockfile
RUN pnpm run -r build
RUN pnpm deploy --filter=app1 --prod /prod/app1
RUN pnpm deploy --filter=app2 --prod /prod/app2

FROM base AS app1
COPY --from=build /prod/app1 /prod/app1
WORKDIR /prod/app1
EXPOSE 8000
CMD [ "pnpm", "start" ]

FROM base AS app2
COPY --from=build /prod/app2 /prod/app2
WORKDIR /prod/app2
EXPOSE 8001
CMD [ "pnpm", "start" ]
```

Run the following commands to build images for app1 and app2:

```sh
docker build . --target app1 --tag app1:latest
docker build . --target app2 --tag app2:latest
```

### Example 3: Build on CI/CD

On CI or CD environments, the BuildKit cache mounts might not be available, because the VM or container is ephemeral and only normal docker cache will work.

So an alternative is to use a typical Dockerfile with layers that are built incrementally, for this scenario, `pnpm fetch` is the best option, as it only needs the lockfile and the project settings, and the layer cache will only be lost when you change the lockfile or the project settings.

`pnpm fetch` only fills the virtual store. A `pnpm install --offline` after copying the sources links the dependencies into `node_modules`. The build stage installs every dependency, and a separate stage installs only the production dependencies, so the final image does not carry the dev dependencies.

This example assumes the project has a `pnpm-workspace.yaml`. If it does not,
use `COPY pnpm-lock.yaml /app/` instead. If you patched any package, also copy
the patches directory before running `pnpm fetch`. Copy the files that
[`include`](./config-includes.md) names in `pnpm-workspace.yaml` there too.

```dockerfile title="Dockerfile"
FROM ghcr.io/pnpm/pnpm:12 AS base
RUN pnpm runtime set node 24 -g
WORKDIR /app

FROM base AS build
COPY pnpm-lock.yaml pnpm-workspace.yaml /app/
RUN pnpm fetch
COPY . /app
RUN pnpm install --offline --frozen-lockfile
RUN pnpm run build

FROM base AS prod-deps
COPY pnpm-lock.yaml pnpm-workspace.yaml /app/
RUN pnpm fetch --prod
COPY . /app
RUN pnpm install --offline --frozen-lockfile --prod

FROM base
COPY package.json /app/
COPY --from=prod-deps /app/node_modules /app/node_modules
COPY --from=build /app/dist /app/dist
EXPOSE 8000
CMD [ "pnpm", "start" ]
```
