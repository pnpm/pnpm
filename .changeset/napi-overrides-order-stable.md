---
"pacquet": patch
---

Fixed the `overrides` block of `pnpm-lock.yaml` being rewritten in a random order on every install performed through `@pnpm/napi`. The recorded overrides now keep the order they were declared in, so repeat installs no longer churn the lockfile.
