---
"@pnpm/plugin-commands-installation": minor
"pnpm": minor
---

It is possible now to update all dependencies except the listed ones using `!`. For instance, update all dependencies, except `lodash`:

```
pnpm update !lodash
```

It also works with pattends, for instance:

```
pnpm update !@babel/*
```

And it may be combined with other patterns:

```
pnpm update @babel/* !@babel/core
```
