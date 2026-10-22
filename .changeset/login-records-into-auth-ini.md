---
"pacquet": patch
---

`pnpm login` saves the token in `<pnpm config>/auth.ini` again. A token saved under `_auth` in the global `config.yaml` also chose where packages were resolved from, so an unscoped login made its registry the default for every project that did not name one. Logging in again removes the old `_auth` token for that registry or scope. `pnpm login --scope` still routes the scope through the global `registries` setting.
