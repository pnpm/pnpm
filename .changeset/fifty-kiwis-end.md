---
"@pnpm/lockfile-utils": major
"@pnpm/resolver-base": major
"@pnpm/npm-resolver": major
"pnpm": patch
---

(Important) Tarball resolutions in `pnpm-lock.yaml` will no longer contain a `registry` field. This field has been unused for a long time. This change should not cause any issues besides backward compatible modifications to the lockfile [#7262](https://github.com/pnpm/pnpm/pull/7262).
