---
"pacquet": patch
---

Resolution no longer depends on the order in which concurrent resolutions of the same package finish. When one package was reached from several places, whichever occurrence happened to walk it first decided the versions its dependencies were recorded at, so repeated installs of the same project could produce different `pnpm-lock.yaml` files.
