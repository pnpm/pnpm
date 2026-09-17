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

A mirror carries the project's own layout below it, so only the host above it changes. Bun had no such setting before. `pnpm pack-app` now downloads the Node.js it embeds through the configured mirror as well.

A tool that publishes several lines of builds takes each one below `mirror`. `channels` sends a named line somewhere else, and leaves the rest where `mirror` puts them. `node-mirror:<channel>` keeps working and names the same thing as an entry under `channels`.
