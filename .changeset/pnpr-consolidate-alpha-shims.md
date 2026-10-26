---
"@pnpm/pnpr": patch
---

Removed `pnpr upgrade-storage`, `pnpr config-check`, CLI flag aliases (`--packument-ttl-secs`, `--no-check-hashes`, `--min-age-secs`, `--max-age-secs`), `PNPR_PACKUMENT_TTL_SECS`, and YAML configuration aliases (`snake_case`, `*_secs`, `backend.postgresql`). Standardized YAML configuration keys on `camelCase` (`maxUsers`, `tokenEnv`, `maxAge`, `syncInterval`, `backend.postgres`).
