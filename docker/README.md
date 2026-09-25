# pnpm base Docker image

Official base image for pnpm, published to GitHub Container Registry.

```
ghcr.io/pnpm/pnpm
```

Based on `debian:stable-slim` with the pnpm standalone binary. Node.js is **not** bundled — install the version you need inside your own image with `pnpm runtime set node <version> -g` (the `-g` flag makes `node` available on `PATH` for subsequent layers and at runtime).

## Tags

| Tag                   | Meaning                                                                 |
| --------------------- | ----------------------------------------------------------------------- |
| `<version>`           | Exact, immutable (e.g. `11.0.0`). Includes prereleases.                 |
| `<major>`             | Tracks the latest stable release within that major (e.g. `11`).         |
| `latest`              | Most recent stable pnpm release. Not updated for prereleases.           |

## Supported platforms

`linux/amd64`, `linux/arm64`.

## Usage

Install Node.js explicitly with `pnpm runtime set`:

```dockerfile
FROM ghcr.io/pnpm/pnpm:latest
RUN pnpm runtime set node 22 -g
WORKDIR /app
COPY . .
RUN pnpm install --frozen-lockfile
CMD ["node", "index.js"]
```

Or let pnpm install Node.js from `devEngines.runtime` in your `package.json`:

```json
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

```dockerfile
FROM ghcr.io/pnpm/pnpm:latest
WORKDIR /app
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
CMD ["pnpm", "start"]
```

## Build locally

```sh
VERSION=11.0.0-rc.2
SHA=$(curl -fsSL "https://github.com/pnpm/pnpm/releases/download/v${VERSION}/pnpm-linux-x64.tar.gz" \
      | sha256sum | awk '{print $1}')
docker buildx build \
  --build-arg PNPM_VERSION=${VERSION} \
  --build-arg PNPM_SHA256_AMD64=${SHA} \
  --platform linux/amd64 \
  --load \
  -t pnpm-test ./docker
docker run --rm pnpm-test pnpm --version
```

## Release

Images are built and pushed by [`.github/workflows/docker.yml`](../.github/workflows/docker.yml) on `release: published`, or manually via `workflow_dispatch`. The build fails if `pnpm --version` in the image doesn't match the `PNPM_VERSION` build-arg.
