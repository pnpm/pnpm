---
"@pnpm/releasing.commands": minor
"pnpm": minor
---

Added a new `pnpm build-sea` command that builds a standalone [Node.js Single Executable Application](https://nodejs.org/api/single-executable-applications.html) from a CommonJS entry file. Targets are specified as `<os>-<arch>[-<libc>]` (e.g. `linux-x64`, `linux-x64-musl`, `macos-arm64`, `win-x64`) and each produces an executable under `dist-sea/<target>/` by default. Requires Node.js v25.5+ to run the injection; an older host downloads Node.js v25 automatically.
