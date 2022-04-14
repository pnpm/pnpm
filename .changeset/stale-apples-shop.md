---
"@pnpm/config": minor
"@pnpm/client": minor
"@pnpm/store-connection-manager": minor
"pnpm": minor
---

New setting added: `git-shallow-hosts`. When cloning repositories from "shallow-hosts", pnpm will use shallow cloning to fetch only the needed commit, not all the history [#4548](https://github.com/pnpm/pnpm/pull/4548).
