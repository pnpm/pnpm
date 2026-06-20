---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed non-deterministic peer resolution that could add or remove an optional transitive peer — for example `@babel/core`, reached through `styled-jsx` — from a package's peer-dependency suffix across otherwise identical installs, churning the lockfile and causing intermittent `pnpm dedupe --check` failures in CI. When a package's children are resolved by one occurrence (the "owner") and reused by a deeper consumer, whether that consumer inherited the owner's missing peers depended on whether the owner's resolution had finished yet — a race under concurrent resolution. The decision is now a function of the dependency graph's structure rather than resolution-completion order.
