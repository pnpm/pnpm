---
"pacquet": patch
---

Sped up installs in large workspaces: the resolver now shares the already-parsed `pnpm-lock.yaml` instead of deep-copying it before every fresh resolution [#14352](https://github.com/pnpm/pnpm/issues/14352).
