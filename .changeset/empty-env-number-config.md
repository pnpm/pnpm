---
"@pnpm/config.reader": patch
"pnpm": patch
---

Ignore empty or whitespace-only numeric `pnpm_config_*` / `npm_config_*` environment variables instead of coercing them to `0`.
