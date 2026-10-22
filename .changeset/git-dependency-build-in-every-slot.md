---
"pacquet": patch
---

pnpm now builds a git-hosted dependency in every new location it links the dependency into, such as a new global virtual store slot or a fresh `node_modules`. When the side-effects cache holds the build output, pnpm restores it from there. Before, only the install that fetched the dependency built it. This also fixes dependencies that are already in the store [#17224](https://github.com/pnpm/pnpm/issues/17224).
