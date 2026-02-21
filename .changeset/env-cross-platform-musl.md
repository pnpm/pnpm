---
"@pnpm/node.fetcher": minor
"@pnpm/plugin-commands-env": minor
"pnpm": minor
---

On systems using the musl C library (e.g. Alpine Linux), `pnpm env` now automatically downloads the musl variant of Node.js from [unofficial-builds.nodejs.org](https://unofficial-builds.nodejs.org).
