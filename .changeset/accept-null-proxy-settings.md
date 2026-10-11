---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm again accepts `httpProxy` and `httpsProxy` set to `null` or written with no value in `pnpm-workspace.yaml` and the global config file, and treats them as unset. Since 11.28.5 these values failed with `ERR_PNPM_INVALID_SETTING`.
