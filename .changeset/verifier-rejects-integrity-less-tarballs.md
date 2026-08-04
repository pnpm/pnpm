---
"pacquet": patch
---

A lockfile entry whose tarball resolution records no `integrity` is now reported by the lockfile-verification gate, before anything is downloaded: every offending entry is listed in one `ERR_PNPM_MISSING_TARBALL_INTEGRITY` error instead of failing the install one fetch at a time after the gate had already passed the lockfile [#13364](https://github.com/pnpm/pnpm/issues/13364). An `integrity: ''` that pins nothing is treated the same as a missing one, and the exemption for git-host archive URLs is now read from the URL rather than the lockfile's own `gitHosted` marker.
