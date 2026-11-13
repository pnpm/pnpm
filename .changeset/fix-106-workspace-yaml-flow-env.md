---
"pacquet": patch
---

`${VAR}` placeholders in `pnpm-workspace.yaml` written with YAML flow syntax now fail on registry URLs with `ERR_PNPM_REGISTRY_ENV_NOT_EXPANDED`. Placeholders in flow-style proxy, `pnprServer`, and `userAgent` settings are now ignored with a warning [zkochan/pnpm-tasks#106](https://github.com/zkochan/pnpm-tasks/issues/106).
