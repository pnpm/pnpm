---
"pacquet": patch
---

`pnpm config set` and `pnpm config delete` no longer destroy repeated keys in `.npmrc`. When the file contains multiple lines for the same key (such as `ca=` for certificate authority lists), an unrelated `pnpm config set` now preserves all of them instead of collapsing to the last value [#14851](https://github.com/pnpm/pnpm/issues/14851).
