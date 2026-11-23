---
"pnpm": patch
---

`pnpm config set` now writes values containing Unicode line separators (U+2028 and U+2029) to `pnpm-workspace.yaml` unchanged, so reading the setting back returns the value that was set.
