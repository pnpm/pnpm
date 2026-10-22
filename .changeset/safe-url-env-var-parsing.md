---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now ignores a `PNPM_CONFIG_*` environment variable for a URL setting, such as `PNPM_CONFIG_REGISTRY`, when its value is not a URL. Such a value used to make every command fail with `Invalid URL`.
