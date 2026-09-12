---
"pacquet": patch
---

`pnpm config set --global node-download-mirrors` no longer rejects the key. The global config file already accepted `nodeDownloadMirrors`, but the command refused to write it [#13611](https://github.com/pnpm/pnpm/issues/13611).
