---
"pacquet": patch
"get-pnpm": patch
---

When the `pnpm` package downloads its native binary on first run, it now reads the registry and credentials from the global `auth.ini`, where `pnpm login` writes them, and from the `_auth` setting of the global `config.yaml`.
