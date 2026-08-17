---
"pacquet": patch
---

Fixed `pnpm install` in CI to use frozen lockfile mode by default when an existing `pnpm-lock.yaml` is non-empty. An outdated lockfile now fails without being rewritten, while projects without a lockfile or with an empty lockfile can still generate one.
