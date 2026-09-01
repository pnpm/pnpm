---
"pacquet": patch
---

Speed up installs in large workspaces by reading and parsing `pnpm-lock.yaml` on a background thread while workspace projects are being discovered, whenever the run is certain to need it (`--frozen-lockfile`, `--force`, or no state from a previous install on disk) [#14352](https://github.com/pnpm/pnpm/issues/14352).
