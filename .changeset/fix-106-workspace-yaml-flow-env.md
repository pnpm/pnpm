---
"pacquet": patch
---

`${VAR}` placeholders in `pnpm-workspace.yaml` written with YAML flow syntax now fail on registry URLs with `ERR_PNPM_REGISTRY_ENV_NOT_EXPANDED`. Placeholders in flow-style proxy, `pnprServer`, and `userAgent` settings are now ignored with a warning [#18074](https://github.com/pnpm/pnpm/pull/18074).
