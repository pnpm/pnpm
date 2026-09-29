---
id: docker
title: Working with Docker
---

:::note

It is impossible to create reflinks or hardlinks between a Docker container and the host filesystem during build time.
The next best thing you can do is using BuildKit cache mount to share cache between builds. Alternatively, you may use
[podman] because it can mount Btrfs volumes during build time.

:::

[podman]: ./podman.md

## Minimizing Docker image size and build time

* Use a small image, e.g. `node:XX-slim`.
* Leverage multi-stage if possible and makes sense.
* Leverage BuildKit cache mounts.

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
FROM node:20-slim AS base
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable
COPY . /app
WORKDIR /app

FROM base AS prod-deps
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --prod --frozen-lockfile

FROM base AS build
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile
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
```

```text title=".dockerignore"
node_modules
.git
.gitignore
*.md
dist
```

```dockerfile title="Dockerfile"
FROM node:20-slim AS base
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable

FROM base AS build
COPY . /usr/src/app
WORKDIR /usr/src/app
RUN --mount=type=cache,id=pnpm,target=/pnpm/store pnpm install --frozen-lockfile
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
