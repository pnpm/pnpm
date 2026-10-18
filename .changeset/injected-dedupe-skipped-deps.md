---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

An injected workspace dependency now becomes a link when its dependencies match the workspace package's own, also on an install that leaves the package's installed dependencies as they are. Before, such an install kept it as a copy [#17094](https://github.com/pnpm/pnpm/issues/17094).
