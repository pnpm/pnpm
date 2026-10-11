---
"pacquet": minor
---

Added the experimental `lockBuildTools` setting. With it, the lockfile records the package manager and Node.js that build each git-hosted dependency, and every install builds the dependency with exactly those tools, verified against the recorded integrity [RFC 21](https://github.com/pnpm/rfcs/pull/21).
