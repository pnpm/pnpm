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

`mirror` is the base a tool's own layout hangs off. `channels` sends one release channel elsewhere and leaves the rest to `mirror`. Both can be set in `pnpm-workspace.yaml` and in the global `config.yaml`.

Bun had no mirror setting before. `pnpm pack-app` downloads the Node.js it embeds through the configured mirror. `node-mirror:<channel>` keeps working and names the same thing as an entry under `channels`.
