# pnpr Docker image

Official image for [`pnpr`](../), the pnpm-compatible npm registry server,
published to GitHub Container Registry.

```text
ghcr.io/pnpm/pnpr
```

Based on `debian:stable-slim` with the standalone `pnpr` binary (static musl
build, the same artifact published to npm). The container runs as a
non-root `pnpr` user and listens on port `7677`.

## Tags

| Tag         | Meaning                                                       |
| ----------- | ------------------------------------------------------------ |
| `<version>` | Exact, immutable (e.g. `0.2.3`). Includes prereleases.       |
| `latest`    | Most recent stable release. Not updated for prereleases.     |

## Supported platforms

`linux/amd64`, `linux/arm64`.

## Usage

```sh
docker run --rm -p 7677:7677 \
  -v pnpr-storage:/pnpr/storage \
  ghcr.io/pnpm/pnpr:latest
```

The default command binds to `0.0.0.0:7677` and stores published packages in
`/pnpr/storage` and the disposable upstream mirror in `/pnpr/cache` (both
declared as volumes). To use a custom config, mount it and point `pnpr` at it:

```sh
docker run --rm -p 7677:7677 \
  -v "$PWD/config.yaml:/pnpr/config.yaml:ro" \
  -v pnpr-storage:/pnpr/storage \
  ghcr.io/pnpm/pnpr:latest --listen 0.0.0.0:7677 --config /pnpr/config.yaml
```

Then point pnpm at it:

```sh
pnpm config set registry http://localhost:7677
```

## Build locally

The build context expects the binary for each target architecture, staged as
`pnpr-amd64` / `pnpr-arm64`. Build one with `cross` (or `cargo` for the host
arch) and drop it in:

The build verifies the binary against a SHA256 checksum before trusting it,
so pass the checksum for the architecture you're building:

```sh
VERSION=0.2.3
cross build -p pnpr --bin pnpr --release --target x86_64-unknown-linux-musl
cp target/x86_64-unknown-linux-musl/release/pnpr pnpr/docker/pnpr-amd64
docker buildx build \
  --build-arg PNPR_VERSION=${VERSION} \
  --build-arg PNPR_SHA256_AMD64=$(shasum -a 256 pnpr/docker/pnpr-amd64 | awk '{print $1}') \
  --platform linux/amd64 \
  --load \
  -t pnpr-test ./pnpr/docker
docker run --rm pnpr-test --version
```

## Release

Images are built and pushed by the `docker` job in
[`.github/workflows/pnpr-release-to-npm.yml`](../../.github/workflows/pnpr-release-to-npm.yml),
which runs after the npm packages are published. The build verifies each
staged binary against the SHA256 checksum pinned by the release job and fails
if `pnpr --version` in the image doesn't match the `PNPR_VERSION` build-arg.
