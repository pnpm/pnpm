---
id: how-peers-are-resolved
title: How peers are resolved
original_id: how-peers-are-resolved
---

One of the very great features of pnpm is that in one project, a specific version of a package will always have
one set of dependencies. There is one exclusion from it though - packages with [peer dependencies](https://docs.npmjs.com/files/package.json#peerdependencies).

Peer dependencies are resolved from dependencies installed higher in the dependency graph.
That means if `foo@1.0.0` has two peers (`bar@^1` and `baz@^1`) then it might have different sets of dependencies
in the same project.

```
- foo-parent-1
  - bar@1.0.0
  - baz@1.0.0
  - foo@1.0.0
- foo-parent-2
  - bar@1.0.0
  - baz@1.1.0
  - foo@1.0.0
```

In the example above, `foo@1.0.0` is installed for `foo-parent-1` and `foo-parent-2`. Both packages have `bar` and `baz`as well, but
they depend on different versions of `baz`. As a result, `foo@1.0.0` has two different sets of dependencies: one with `baz@1.0.0`
and the other one with `baz@1.1.0`. In order to support these use cases, pnpm has to hard link `foo@1.0.0` as many times as many different dependency sets it has.

Normally, if a package does not have peer dependencies, it is hard linked to a `node_modules` folder next to symlinks of its dependencies.

![](/img/how-peers-are-resolved/1.png)

However, if `foo` has peer dependencies, there cannot be one single set of dependencies for it, so
we create different sets, for different peer dependency resolutions:

![](/img/how-peers-are-resolved/2.png)

We create symlinks either to the `foo` that is inside `bar@1.0.0+baz@1.0.0/node_modules` or to the one in `bar@1.0.0+baz@1.1.0/node_modules`.
As a consequence, the Node.js module resolver algorithm will find the correct peers.

*If the resolved peer is a direct dependency of the project*, it is not grouped separately with the dependent package.
This is done to make it easier to make predictable and fast named (`pnpm i foo`) and general (`pnpm i`) installations.
So if the project dependends on `bar@1.0.0`, the dependencies from our example will be grouped like this:

![](/img/how-peers-are-resolved/3.png)

*If a package has no peer dependencies but has dependencies with peers that are resolved higher in the graph*, then
that transitive package can appear in the project with different sets of dependencies. For instance, there's package `a@1.0.0`
with a single dependency `framework@1.0.0`. `framework@1.0.0` has a peer dependency `plugin@^1`. `a@1.0.0` will never resolve the
peers of `framework@1.0.0`, so it becomes dependent from the peers of `framework@1.0.0` as well.

Here's how it will look like in `node_modules/.registry.npmjs.org`, in case if `a@1.0.0` will need to appear twice in the project's
`node_modules`, once resolved with `plugin@1.0.0` and once with `plugin@1.1.0`.

![](/img/how-peers-are-resolved/4.png)
