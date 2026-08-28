---
"pacquet": minor
---

`pnpm login` and `pnpm adduser` now record the granted token in the global `config.yaml`, under the `_auth` setting, with `--scope`'s scope routed to that registry under `registries`. `pnpm logout` removes it from there, and still from an `auth.ini` an earlier version wrote. Tokens already in `auth.ini` keep working.
