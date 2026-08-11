---
"pacquet": patch
---

Fixed dependency resolution letting the order in which concurrent resolutions finished decide the outcome. When one package was reached from several places, whichever occurrence got there first decided the versions its dependencies were recorded at, so repeated installs of the same project could produce different `pnpm-lock.yaml` files.
