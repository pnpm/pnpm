---
"pacquet": minor
---

`tools` names the programs pnpm downloads, and `mirror` says where each one comes from.

```yaml
tools:
  node:
    mirror: https://mirror.example.com/node/download
  bun:
    mirror: https://mirror.example.com/bun
  python:
    mirror: https://mirror.example.com/python-build-standalone/releases
```

A mirror carries the project's own layout below it, so only the host above it changes. Bun had no such setting before. `pnpm pack-app` now downloads the Node.js it embeds through the configured mirror as well.

`node-mirror:<channel>` keeps working and keeps naming one release channel, so it decides the channel it names and `tools.node.mirror` supplies the rest.
