# Shared store

A shared storage saves tons of space! However, a shared store is used by default only on Node.js >= v6.3.0. The reason for that is because
it uses the `--preserve-symlinks` option (read more about it: [nodejs/node#3402](https://github.com/nodejs/node/issues/3402)).

For pre 6.3.0 pnpm creates a separate store for every package in it's `node_modules` folder. However, it is possible to change the value of
the `store-path` config key and force `pnpm` to use a shared storage.

The easiest way to do it is via the command line:

```sh
pnpm config set store-path ~/.store
```

**Disclaimer!** Packages like babel that heavily rely on peer dependencies won't work with a shared storage without the `--preserve-symlinks` flag.
