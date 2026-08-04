---
"pacquet": patch
---

Concurrent commands in a repository that pins `packageManager` no longer race while installing the pinned pnpm version on a cold cache [#13322](https://github.com/pnpm/pnpm/issues/13322). A task runner spawning several `pnpm run` children at once could previously fail with "failed to remove existing directory … prior to swap", or leave a child looking for a binary another process had just unlinked.
