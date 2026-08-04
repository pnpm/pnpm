---
"pacquet": patch
---

With `excludeLinksFromLockfile` enabled, a `link:` dependency pointing inside the workspace is no longer treated as an external link when it resolves a peer dependency, so the peer suffixes it produces stay identical to an install with the setting off. Injected (`file:`) workspace dependencies are no longer affected by the setting either [#13556](https://github.com/pnpm/pnpm/issues/13556).
