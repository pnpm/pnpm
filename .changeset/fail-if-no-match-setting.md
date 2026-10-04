---
"pacquet": patch
---

The `failIfNoMatch` setting in `pnpm-workspace.yaml` is honored again. A filter that matches no workspace project now exits with code 1 when the setting is `true`, and `--no-fail-if-no-match` turns it off for one command [#16577](https://github.com/pnpm/pnpm/issues/16577).
