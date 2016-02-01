# npm

pnpm is a reimplementation of `npm install`.

## Why's pnpm faster?

npm performs its action in stages. It performs one stage for all dependencies in the tree before moving onto the next stage. In general, these are:

0. __Resolving__: get package data and what dependencies it has. (many requests, low bandwidth)
0. __Fetching__: fetch module contents (less requests, high bandwidth)
0. __Extracting__: extracting module contents from .tar.gz (no network IO)
0. __Building__: build compiled modules (no network IO)

pnpm will eagerly move onto the next stage for a module even if other modules are stuck in earlier stages. This allows pnpm to more efficiently manage network IO: for instance, it can build compiled modules in the background while resolutions/fetches are still happening.

There's more to it than this, but this is one of the big reasons.

## Why's npm2 faster than npm3?

_(This article is a stub. you can help by expanding it.)_

## npm3's flat tree

npm now maintains a [flattened dependency tree](https://github.com/npm/npm/issues/6912) as of version 3. This leads to less disk space bloat, with a messy `node_modules` directory as a side effect.

On the other hand, pnpm manages `node_modules` as an addressable storage in its [store layout](store-layout.md). This nets you the benefits of less disk space usage, while keeping your `node_modules` clean.

## Will pnpm replace npm?

**No!** pnpm is not a _replacement_ for npm; rather, think of it as a _supplement_ to npm. It's simply a rewrite of the `npm install` command that uses an alternate way to store your modules. It won't reimplement other things npm is used for (publishing, node_modules management, and so on).

## Limitations

- Windows is [not fully supported](https://github.com/rstacruz/pnpm/issues/6) (yet).
- You can't install from [shrinkwrap][] (yet).
- Peer dependencies are a little trickier to deal with.
- You can't publish npm modules with `bundleDependencies` managed by pnpm.
- Things not ticked off in the [to do list](roadmap.md) are obviously not feature-complete.

Got an idea for workarounds for these issues? [Share them.](https://github.com/rstacruz/pnpm/issues/new)

[shrinkwrap]: https://docs.npmjs.com/cli/shrinkwrap
[npm ls]: https://docs.npmjs.com/cli/ls
[npm prune]: https://docs.npmjs.com/cli/prune
[npm dedupe]: https://docs.npmjs.com/cli/dedupe

