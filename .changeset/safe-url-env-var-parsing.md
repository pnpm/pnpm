---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm no longer crashes with `Invalid URL` when a `PNPM_CONFIG_*` environment variable for a URL setting, such as `PNPM_CONFIG_REGISTRY`, holds a value that is not a URL. pnpm ignores the value, as it does with other environment values it cannot parse.
