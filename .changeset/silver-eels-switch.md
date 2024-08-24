---
"pnpm": minor
---

**Minor breaking change.** This change might result in resolving your peer dependencies slightly differently but we don't expect it to introduce issues.

We had to optimize how we resolve peer dependencies in order to fix some [infinite loops and out-of-memory errors during peer dependencies resolution](https://github.com/pnpm/pnpm/issues/8370).

When a peer dependency is a prod dependency somewhere in the dependency graph (with the same version), pnpm will resolve the peers of that peer dependency in the same way across the subgraph.

For example, we have `react-dom` in the peer deps of the `form` and `button` packages. `card` has `react-dom` and `react` as regular dependencies and `card` is a dependency of `form`.

These are the direct dependencies of our example project:

```
form
react@16
react-dom@16
```

These are the dependencies of card:

```
button
react@17
react-dom@16
```

When resolving peers, pnpm will not re-resolve `react-dom` for `card`, even though `card` shadows `react@16` from the root with `react@17`. So, all 3 packages (`form`, `card`, and `button`) will use `react-dom@16`, which in turn uses `react@16`. `form` will use `react@16`, while `card` and `button` will use `react@17`.

Before this optimization `react-dom@16` was duplicated for the `card`, so that `card` and `button` would use a `react-dom@16` instance that uses `react@17`.

Before the change:

```
form
-> react-dom@16(react@16)
-> react@16
card
-> react-dom@16(react@17)
-> react@17
button
-> react-dom@16(react@17)
-> react@17
```

After the change

```
form
-> react-dom@16(react@16)
-> react@16
card
-> react-dom@16(react@16)
-> react@17
button
-> react-dom@16(react@16)
-> react@17
```

