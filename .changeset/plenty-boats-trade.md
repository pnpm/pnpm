---
"@pnpm/resolve-dependencies": major
"pnpm": major
---

Peer dependencies of peer dependencies are now resolved correctly. When peer dependencies have peer dependencies of their own, the peer dependencies are grouped with their own peer dependencies before being linked to their dependents.

For instance, if `card` has `react` in peer dependencies and `react` has `typescript` in its peer dependencies, then the same version of `react` may be linked from different places if there are multiple versions of `typescript`. For instance:

```
project1/package.json
{
  "dependencies": {
    "card": "1.0.0",
    "react": "16.8.0",
    "typescript": "7.0.0"
  }
}
project2/package.json
{
  "dependencies": {
    "card": "1.0.0",
    "react": "16.8.0",
    "typescript": "8.0.0"
  }
}
node_modules
  .pnpm
    card@1.0.0(react@16.8.0(typescript@7.0.0))
      node_modules
        card
        react --> ../../react@16.8.0(typescript@7.0.0)/node_modules/react
    react@16.8.0(typescript@7.0.0)
      node_modules
        react
        typescript --> ../../typescript@7.0.0/node_modules/typescript
    typescript@7.0.0
      node_modules
        typescript
    card@1.0.0(react@16.8.0(typescript@8.0.0))
      node_modules
        card
        react --> ../../react@16.8.0(typescript@8.0.0)/node_modules/react
    react@16.8.0(typescript@8.0.0)
      node_modules
        react
        typescript --> ../../typescript@8.0.0/node_modules/typescript
    typescript@8.0.0
      node_modules
        typescript
```

In the above example, both projects have `card` in dependencies but the projects use different versions of `typescript`. Hence, even though the same version of `card` is used, `card` in `project1` will reference `react` from a directory where it is placed with `typescript@7.0.0` (because it resolves `typescript` from the dependencies of `project1`), while `card` in `project2` will reference `react` with `typescript@8.0.0`.

Related issue: [#7444](https://github.com/pnpm/pnpm/issues/7444).
Related PR: [#7606](https://github.com/pnpm/pnpm/pull/7606).
