---
"pacquet": minor
---

`tools` names the programs pnpm downloads, and `mirror` says where each one comes from.

```yaml
tools:
  node:
    mirror: https://mirror.example.com/node/download
    channels:
      nightly: https://nightly.example.com/
  bun:
    mirror: https://mirror.example.com/bun
  python:
    mirror: https://mirror.example.com/python-build-standalone/releases
```

`node`, `bun` and `python` can be named. Any other tool is refused.

`mirror` is the base a tool's own layout hangs off.

`channels` sends one release channel elsewhere. Every channel it does not name is left to `mirror`. Only `node` publishes channels, so naming them for another tool is refused.

Set it in the global `config.yaml` or in `PNPM_CONFIG_TOOLS`. A `pnpm-workspace.yaml` that names a tool mirror is ignored.

`tools.python.mirror` replaces `python.downloadUrl`, which is gone. Bun had no mirror setting before. `pnpm pack-app` downloads the Node.js it embeds through the configured mirror. `node-mirror:<channel>` keeps working and names the same thing as an entry under `channels`.
