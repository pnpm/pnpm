---
"pacquet": patch
---

A lockfile entry for a git-hosted archive that records no `integrity` installs again instead of failing with `ERR_PNPM_MISSING_TARBALL_INTEGRITY`. Older pnpm versions wrote that shape for dependencies like `"ci-info": "watson/ci-info#f43f6a1c…"`, so any committed lockfile still carrying one could not be installed [#13308](https://github.com/pnpm/pnpm/issues/13308). The archive URL pins a full commit SHA, and pnpm fetches it without an integrity check.

Every other remote tarball still has to carry an `integrity`, and the refusal now points at the repair: `pnpm clean --lockfile` followed by `pnpm install`.

Error output no longer repeats the same message once per level of the internal error chain.
