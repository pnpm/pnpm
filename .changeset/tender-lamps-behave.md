---
"pnpm": minor
---

Added support for `finders` [#9946](https://github.com/pnpm/pnpm/pull/9946).

In the past, `pnpm list` and `pnpm why` could only search for dependencies by **name** (and optionally version). For example:

```
pnpm why minimist
```

prints the chain of dependencies to any installed instance of `minimist`:

```
verdaccio 5.20.1
├─┬ handlebars 4.7.7
│ └── minimist 1.2.8
└─┬ mv 2.1.1
  └─┬ mkdirp 0.5.6
    └── minimist 1.2.8
```

What if we want to search by **other properties** of a dependency, not just its name? For instance, find all packages that have `react@17` in their peer dependencies?

This is now possible with "finder functions". Finder functions can be declared in `.pnpmfile.cjs` and invoked with the `--find-by=<function name>` flag when running `pnpm list` or `pnpm why`.

Let's say we want to find any dependencies that have React 17 in peer dependencies. We can add this finder to our `.pnpmfile.cjs`:

```js
module.exports = {
  finders: {
    react17: (ctx) => {
      return ctx.readManifest().peerDependencies?.react === '^17.0.0'
    }
  }
}
```

Now we can use this finder function by running:

```
pnpm why --find-by=react17
```

pnpm will find all dependencies that have this React in peer dependencies and print their exact locations in the dependency graph.

```
@apollo/client 4.0.4
├── @graphql-typed-document-node/core 3.2.0
└── graphql-tag 2.12.6
```

It is also possible to print out some additional information in the output by returning a string from the finder. For example, with the following finder:

```js
module.exports = {
  finders: {
    react17: (ctx) => {
      const manifest = ctx.readManifest()
      if (manifest.peerDependencies?.react === '^17.0.0') {
        return `license: ${manifest.license}`
      }
      return false
    }
  }
}
```

Every matched package will also print out the license from its `package.json`:

```
@apollo/client 4.0.4
├── @graphql-typed-document-node/core 3.2.0
│   license: MIT
└── graphql-tag 2.12.6
    license: MIT
```
