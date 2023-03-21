---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

When publish some package throws an error, the exit code should be non-zero [#5528](https://github.com/pnpm/pnpm/issues/5528).
