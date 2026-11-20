---
"pacquet": patch
---

Sped up filtered dependency installs such as `pnpm install --prod --frozen-lockfile` on v9 lockfiles with hashed peer suffixes. pnpm skips peer recovery during dependency filtering for snapshots without a matching optional peer entry [#18276](https://github.com/pnpm/pnpm/issues/18276).
