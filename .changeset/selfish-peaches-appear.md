---
"@pnpm/config": patch
"pnpm": patch
---

`extend-node-path` is `true` by default. It was set to `false` in v7.29.2 but it appears that it was a breaking change [#6213](https://github.com/pnpm/pnpm/issues/6213).
