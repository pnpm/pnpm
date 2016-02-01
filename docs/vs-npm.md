# npm

pnpm is made as a more performance

## Why is npm2 faster than npm3?

_(This article is a stub. you can help by expanding it.)_

## Why is pnpm faster than npm?

npm performs its action in stages. That is, it performs one stage for all dependencies in the tree, then moves onto the next stage. In general, these are:

- Resolving: get package data and what dependencies it has. (high requests, low bandwidth)
- Fetching: fetch module contents (low requests, high bandwidth)
- Building: build compiled modules (no network IO)

On the other hand, pnpm will eagerly move onto the next stage for a module even if other modules are stuck in earlier stages. This allows you to pnpm to more efficiently manage network IO: for instance, it can build compiled modules in the background while resolutions/fetches are still happening.

There's more to it than this, but this is one of the big reasons.

## Will pnpm replace npm?

**No!** pnpm is not a _replacement_ for npm; rather, think of it as a _supplement_ to npm.

It's simply a rewrite of the `npm install` command that uses an alternate way to store your modules. It won't reimplement other things npm is used for (publishing, node_modules management, and so on).

## Limitations

- Windows is [not fully supported](https://github.com/rstacruz/pnpm/issues/6) (yet).
- You can't install from [shrinkwrap][] (yet).
- Things not ticked off in the [to do list](../README.md#preview-release) are obviously not feature-complete.

Got an idea for workarounds for these issues? [Share them.](https://github.com/rstacruz/pnpm/issues/new)

[shrinkwrap]: https://docs.npmjs.com/cli/shrinkwrap
[npm ls]: https://docs.npmjs.com/cli/ls
[npm prune]: https://docs.npmjs.com/cli/prune
[npm dedupe]: https://docs.npmjs.com/cli/dedupe

