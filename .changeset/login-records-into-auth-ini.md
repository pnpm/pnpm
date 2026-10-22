---
"pacquet": patch
---

`pnpm login` saves the token in `<pnpm config>/auth.ini` again. Since v12.1.0, an unscoped login had made its registry the default for every project that did not name a registry. A login now also removes the token an earlier login saved for that registry or scope under `_auth` in the global `config.yaml`. `pnpm login --scope` still routes the scope through the global `registries` setting.
