---
id: bin
title: pnpm bin
---

Prints the directory into which the executables of dependencies are linked, the project's `node_modules/.bin`, or `<modulesDir>/.bin` with a custom [`modulesDir`](../settings/node-modules.md#modulesdir).

```sh
$ pnpm bin
/path/to/project/node_modules/.bin
```

## Options

### --global, -g

Prints the location of the globally installed executables.
