---
id: installation
title: Installation
---

pnpr is distributed on npm as [`@pnpm/pnpr`](https://www.npmjs.com/package/@pnpm/pnpr).
The npm wrapper requires Node.js 18 or newer. Install it globally:

```sh
pnpm add -g @pnpm/pnpr@next
```

Until the first stable release, every pnpr version is a prerelease published
under the `next` dist-tag, so install `@pnpm/pnpr@next`.

The wrapper package resolves to the native binary published under
`@pnpm/pnpr.<platform>-<arch>` (e.g. `@pnpm/pnpr.linux-x64`). Prebuilt binaries
are available for:

- `linux-x64`, `linux-arm64`
- `linux-x64-musl`, `linux-arm64-musl`
- `darwin-x64`, `darwin-arm64`
- `win32-x64`, `win32-arm64`

Only the binary matching your platform is downloaded.

## Container image

pnpr is also published as the `ghcr.io/pnpm/pnpr` container image:

```sh
docker run -p 127.0.0.1:7677:7677 -v pnpr-storage:/pnpr/storage ghcr.io/pnpm/pnpr:<version>
```

- The entrypoint is `pnpr`. Arguments after the image name replace the default
  command, `serve --listen 0.0.0.0:7677 --storage /pnpr/storage --cache /pnpr/cache`.
- `/pnpr/storage` and `/pnpr/cache` are volumes. Only `/pnpr/storage` holds
  data that must survive.
- The default command passes `--storage /pnpr/storage`, so the bundled config
  keeps `htpasswd` and `tokens.db` in that volume. Pass the same flag when you
  replace the command, for example to create an account:

  ```sh
  docker run --rm -it -v pnpr-storage:/pnpr/storage ghcr.io/pnpm/pnpr:<version> add-user alice --storage /pnpr/storage
  ```
- The server runs as the non-root `pnpr` user.
- The example publishes the port on loopback only. pnpr serves plain HTTP, so
  put a [TLS-terminating proxy](tls-and-reverse-proxy.md) in front before
  clients on other machines log in.

## Verify the installation

```sh
pnpr --version
```

## Next steps

Continue to the [Quick start](quick-start.md) to run the server and point a
client at it.
