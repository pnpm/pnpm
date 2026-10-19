---
"pacquet": minor
---

Added the `checkPeerDependencies` setting. Set it to `false` to skip the peer dependency check that runs at the end of `pnpm install` and other commands that resolve dependencies. `pnpm peers check` still reports peer dependency issues.
