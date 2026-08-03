---
"@pnpm/config.reader": minor
"pnpm": minor
"pacquet": minor
---

`nodeDownloadMirrors` can now be set in the global config file (`config.yaml`) and through the `PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS` environment variable, so a Node.js download mirror can be configured once for a machine instead of in every workspace [#12124](https://github.com/pnpm/pnpm/issues/12124).

```sh
PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS='{"release":"https://npmmirror.com/mirrors/node/"}'
```
