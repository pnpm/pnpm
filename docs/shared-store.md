# Shared store

**Disclaimer!** This will work only on Node.js >= v6.3.0, because it uses the `--preserve-symlinks` option (read more about it: [nodejs/node#3402](https://github.com/nodejs/node/issues/3402)).

By default, `pnpm` creates a separate store for every package in it's `node_modules` folder. However, it is possible to change the value of the `store_path` config key and force `pnpm` to use a shared storage.

The easiest way to do it is via the command line:

```sh
pnpm config set store_path ~/.store
```

A shared storage can save tons of space!
