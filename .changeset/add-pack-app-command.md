---
"@pnpm/releasing.commands": minor
"pnpm": minor
---

Added a new `pnpm pack-app` command that packs a CommonJS entry file into a standalone executable for one or more target platforms, using the [Node.js Single Executable Applications](https://nodejs.org/api/single-executable-applications.html) API under the hood. Targets are specified as `<os>-<arch>[-<libc>]` (e.g. `linux-x64`, `linux-x64-musl`, `macos-arm64`, `win-x64`) and each produces an executable under `dist-app/<target>/` by default. Requires Node.js v25.5+ to perform the injection; an older host downloads Node.js v25 automatically.
