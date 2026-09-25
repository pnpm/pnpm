---
"pacquet": patch
---

`pnpm setup` on Windows no longer fails when an unrelated environment variable has a name containing a non-ASCII character. It used to panic instead of skipping that variable [#15684](https://github.com/pnpm/pnpm/issues/15684).
