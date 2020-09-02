---
"@pnpm/resolve-dependencies": minor
---

We are building the dependency tree only until there are new packages or the packages repeat in a unique order. This is needed later during peer dependencies resolution.

So we resolve `foo > bar > qar > foo`.
But we stop on `foo > bar > qar > foo > qar`.
In the second example, there's no reason to walk qar again when qar is included the first time, the dependencies of foo are already resolved and included as parent dependencies of qar. So during peers resolution, qar cannot possibly get any new or different peers resolved, after the first ocurrence.

However, in the next example we would analyze the second qar as well, because zoo is a new parent package:
`foo > bar > qar > zoo > qar`
