---
"@pnpm/config.package-is-installable": minor
"@pnpm/config.reader": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.deps-restorer": minor
"@pnpm/installing.package-requester": minor
"@pnpm/store.connection-manager": minor
"@pnpm/store.controller": minor
"pnpm": minor
---

Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).
