---
"@pnpm/node.fetcher": minor
"@pnpm/plugin-commands-env": minor
"pnpm": minor
---

`pnpm env add/use/remove` now support `--platform`, `--arch`, and `--libc` options for cross-platform Node.js downloads. For example, to download a musl (Alpine Linux) build:

```
pnpm env add --global --platform linux --arch x64 --libc musl 22
```

On systems that use the musl C library (e.g. Alpine Linux), the musl variant is auto-detected so `pnpm env` works out of the box without any extra flags.

`pnpm env add --json` outputs a JSON array of installed entries, each with `version` and `dir` fields.
