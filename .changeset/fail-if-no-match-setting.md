---
"pacquet": patch
---

pnpm now reads the `failIfNoMatch` setting from `pnpm-workspace.yaml`, so a filter that matches no workspace project exits with code 1 when the setting is `true`. The new `--no-fail-if-no-match` flag turns the setting off for one command [#16577](https://github.com/pnpm/pnpm/issues/16577).
