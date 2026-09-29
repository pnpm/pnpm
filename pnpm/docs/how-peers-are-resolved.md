---
id: how-peers-are-resolved
title: How peers are resolved
---

One of the best features of pnpm is that in one project, a specific version of a
package will always have one set of dependencies. There is one exception from
this rule, though - packages with [peer dependencies].

[peer dependencies]: https://docs.npmjs.com/cli/v10/configuring-npm/package-json#peerdependencies

Peer dependencies are resolved from dependencies installed higher in the
dependency graph, since they share the same version as their parent. That means
that if `foo@1.0.0` has two peers (`bar@^1` and `baz@^1`) then it might have
multiple different sets of dependencies in the same project.

```text
- foo-parent-1
  - bar@1.0.0
  - baz@1.0.0
  - foo@1.0.0
- foo-parent-2
  - bar@1.0.0
  - baz@1.1.0
  - foo@1.0.0
```

In the example above, `foo@1.0.0` is installed for `foo-parent-1` and
`foo-parent-2`. Both packages have `bar` and `baz` as well, but they depend on
different versions of `baz`. As a result, `foo@1.0.0` has two different sets of
dependencies: one with `baz@1.0.0` and the other one with `baz@1.1.0`. To
support these use cases, pnpm has to hard link `foo@1.0.0` as many times as
there are different dependency sets.

Normally, if a package does not have peer dependencies, it is hard linked to a
`node_modules` folder next to symlinks of its dependencies, like so:

```text
node_modules
└── .pnpm
    ├── foo@1.0.0
    │   └── node_modules
    │       ├── foo
    │       ├── qux   -> ../../qux@1.0.0/node_modules/qux
    │       └── plugh -> ../../plugh@1.0.0/node_modules/plugh
    ├── qux@1.0.0
    ├── plugh@1.0.0
```

However, if `foo` has peer dependencies, there may be multiple sets of
dependencies for it, so we create different sets for different peer dependency
resolutions:

```text
node_modules
└── .pnpm
    ├── foo@1.0.0_bar@1.0.0+baz@1.0.0
    │   └── node_modules
    │       ├── foo
    │       ├── bar   -> ../../bar@1.0.0/node_modules/bar
    │       ├── baz   -> ../../baz@1.0.0/node_modules/baz
    │       ├── qux   -> ../../qux@1.0.0/node_modules/qux
    │       └── plugh -> ../../plugh@1.0.0/node_modules/plugh
    ├── foo@1.0.0_bar@1.0.0+baz@1.1.0
    │   └── node_modules
    │       ├── foo
    │       ├── bar   -> ../../bar@1.0.0/node_modules/bar
    │       ├── baz   -> ../../baz@1.1.0/node_modules/baz
    │       ├── qux   -> ../../qux@1.0.0/node_modules/qux
    │       └── plugh -> ../../plugh@1.0.0/node_modules/plugh
    ├── bar@1.0.0
    ├── baz@1.0.0
    ├── baz@1.1.0
    ├── qux@1.0.0
    ├── plugh@1.0.0
```

We create symlinks either to the `foo` that is inside
`foo@1.0.0_bar@1.0.0+baz@1.0.0` or to the one in
`foo@1.0.0_bar@1.0.0+baz@1.1.0`.
As a consequence, the Node.js module resolver will find the correct peers.

*If a package has no peer dependencies but has dependencies with peers that are
resolved higher in the graph*, then that transitive package can appear in the
project with different sets of dependencies. For instance, there's package
`a@1.0.0` with a single dependency `b@1.0.0`. `b@1.0.0` has a peer dependency
`c@^1`. `a@1.0.0` will never resolve the peers of `b@1.0.0`, so it becomes
dependent from the peers of `b@1.0.0` as well.

Here's how that structure will look in `node_modules`. In this example,
`a@1.0.0` will need to appear twice in the project's `node_modules` - resolved
once with `c@1.0.0` and again with `c@1.1.0`.

```text
node_modules
└── .pnpm
    ├── a@1.0.0_c@1.0.0
    │   └── node_modules
    │       ├── a
    │       └── b -> ../../b@1.0.0_c@1.0.0/node_modules/b
    ├── a@1.0.0_c@1.1.0
    │   └── node_modules
    │       ├── a
    │       └── b -> ../../b@1.0.0_c@1.1.0/node_modules/b
    ├── b@1.0.0_c@1.0.0
    │   └── node_modules
    │       ├── b
    │       └── c -> ../../c@1.0.0/node_modules/c
    ├── b@1.0.0_c@1.1.0
    │   └── node_modules
    │       ├── b
    │       └── c -> ../../c@1.1.0/node_modules/c
    ├── c@1.0.0
    ├── c@1.1.0
```

## Cyclic dependencies

Added in: v12.0.0-rc.5 (pnpm v12 only)

Packages may depend on each other in a cycle: `a` depends on `b`, and `b` — directly, or through further packages — depends back on `a`. When a package inside such a cycle has peer dependencies, there is no single point "higher in the graph" to resolve them from, because entering the cycle at `a` and entering it at `b` lead to different parents.

pnpm cuts every cycle at a fixed place. The packages that form a cycle are ordered by their package IDs, and the edges that close it are cut at the same point no matter where the installation walks into it. The cut edge is still a real dependency — it is resolved against an occurrence of its target taken at the project level — but the walk never travels around the cycle, so a package inside a cycle is no longer resolved once per path that reaches it. Peer dependencies of packages inside a cycle still resolve to the nearest match along that order.

Because the cut no longer depends on the path the installation takes, the lockfile is a function of the dependency graph alone. Repeated installs, reordered `packages` globs in `pnpm-workspace.yaml`, and reordered entries in `package.json` all produce a byte-identical lockfile. Projects with cycle-heavy dependency graphs also end up with a smaller lockfile, since packages inside a cycle no longer get a separate peer variant per path that reaches them.

Existing lockfiles keep working: [`--frozen-lockfile`](./cli/install.md#--frozen-lockfile) installs consume them unchanged, and installs that skip resolution leave them untouched. The first install that re-resolves — after a dependency change, for instance — re-keys the peer variants of cyclic packages once, which shows up as a one-time lockfile diff.

In pnpm v11, where the cut depends on the order the graph is walked, the same dependencies can produce different lockfiles depending on the order projects and dependencies are listed in.
