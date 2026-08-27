---
"pacquet": minor
---

`pnpm login` and `pnpm adduser` now record what they were granted in the global `config.yaml` instead of `auth.ini`: the token under `_auth`, keyed by registry and scope, and `--scope`'s scope routed to that registry under `registries`. Both are settings you can read and edit like any other, and `pnpm config list` keeps the token out of its output. `pnpm logout` removes the credential from `config.yaml`, and still from an `auth.ini` an earlier version wrote; tokens already in `auth.ini` keep working.
