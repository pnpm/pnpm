---
"pacquet": patch
---

Fixed `pnpm run "/pattern/"` running matching scripts one at a time in a single project. Matching scripts now run concurrently up to `workspaceConcurrency`, and their output is prefixed so concurrent lines remain distinguishable [pnpm discussion 14357](https://github.com/orgs/pnpm/discussions/14357).
