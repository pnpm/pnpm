---
"pacquet": patch
---

`pnpm install --frozen-lockfile` no longer waits for a Node.js version check it does not use before it exits. With Homebrew's Node.js, a frozen install with nothing to do can be about 3x faster.
