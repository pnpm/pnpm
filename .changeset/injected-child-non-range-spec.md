---
"pacquet": patch
---

`pnpm install` now resolves a git, tarball, or tag dependency of an injected workspace package again when its spec changes. Before, it kept the old version.
