---
"pacquet": patch
---

Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` corrupting workflow YAML or skipping actions referenced through YAML anchors, aliases, or merge keys.
