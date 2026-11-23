---
"@pnpm/pnpr": patch
---

pnpr now treats an empty `XDG_CONFIG_HOME` or `LOCALAPPDATA` as unset when it looks for its global `config.yaml`.
