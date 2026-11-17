---
"pacquet": patch
---

`pnpm install --frozen-lockfile` no longer waits for an unused Node.js version check before it exits. A frozen install with nothing to do is about 3x faster with Homebrew's Node.js.
