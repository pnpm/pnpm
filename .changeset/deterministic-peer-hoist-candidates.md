---
"pacquet": patch
---

Fixed non-deterministic resolution on multi-project workspaces: two consecutive installs of the same inputs could bind peer-suffixed packages to different (still valid) providers, rewriting `pnpm-lock.yaml` on every install [#13567](https://github.com/pnpm/pnpm/issues/13567).
