---
"pacquet": patch
---

`pnpm install` with `nodeLinker: hoisted` applies a patch once to each copy of a patched dependency in a workspace. The patch used to reach the same directory twice when two projects' copies shared one directory through a symlink, leaving the patched content duplicated [#7565](https://github.com/pnpm/pnpm/issues/7565).
