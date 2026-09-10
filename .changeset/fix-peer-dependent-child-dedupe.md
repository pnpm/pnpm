---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

Fixed an issue where packages whose child dependencies carry peer-dependent suffixes (such as `next` depending on `styled-jsx` with an optional `babel-plugin-macros` peer) were not deduplicated when one workspace project satisfied the optional peer and another did not [#14800](https://github.com/pnpm/pnpm/issues/14800).
