---
"@pnpm/config.reader": patch
"pnpm": patch
---

Empty or whitespace-only numeric `pnpm_config_*` environment variables no longer override configured values or defaults.
