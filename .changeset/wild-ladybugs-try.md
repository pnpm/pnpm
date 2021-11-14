---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Downgrading `p-memoize` to v4.0.1. pnpm v6.22.0 started to print the next warning [#3989](https://github.com/pnpm/pnpm/issues/3989):

```
(node:132923) TimeoutOverflowWarning: Infinity does not fit into a 32-bit signed integer.
```
