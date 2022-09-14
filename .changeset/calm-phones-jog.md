---
"@pnpm/matcher": minor
"pnpm": minor
---

Now it is possible to exclude packages from hoisting by prepending a `!` to the pattern. This works with both the `hoist-pattern` and `public-hoist-pattern` settings. For instance:

```
public-hoist-pattern[]='*types*'
public-hoist-pattern[]='!@types/react'

hoist-pattern[]='*eslint*'
hoist-pattern[]='!*eslint-plugin*'
```

Ref [#5272](https://github.com/pnpm/pnpm/issues/5272)
