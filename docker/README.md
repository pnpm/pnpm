# pnpm base Docker image

Official base image for pnpm, published to GitHub Container Registry.

```
ghcr.io/pnpm/pnpm
```

Based on `debian:stable-slim` with the pnpm standalone binary. Node.js is **not** bundled — install the version you need inside your own image with `pnpm runtime set node <version>`.

## Tags

| Tag                   | Meaning                                                                 |
| --------------------- | ----------------------------------------------------------------------- |
| `<version>`           | Exact, immutable (e.g. `11.0.0`). Includes prereleases.                 |
| `<major>`             | Tracks the latest stable release within that major (e.g. `11`).         |
| `latest`              | Most recent stable pnpm release. Not updated for prereleases.           |

## Supported platforms

`linux/amd64`, `linux/arm64`.

## Usage

```dockerfile
FROM ghcr.io/pnpm/pnpm:latest
RUN pnpm runtime set node 22
WORKDIR /app
COPY . .
RUN pnpm install --frozen-lockfile
CMD ["node", "index.js"]
```

## Build locally

```sh
docker buildx build \
  --build-arg PNPM_VERSION=11.0.0-rc.2 \
  --platform linux/amd64 \
  -t pnpm-test ./docker
docker run --rm pnpm-test pnpm --version
```

## Release

Images are built and pushed by [`.github/workflows/docker.yml`](../.github/workflows/docker.yml) on `release: published`, or manually via `workflow_dispatch`. The build fails if `pnpm --version` in the image doesn't match the `PNPM_VERSION` build-arg.
