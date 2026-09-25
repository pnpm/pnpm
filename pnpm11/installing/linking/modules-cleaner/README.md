# @pnpm/installing.linking.modules-cleaner

> Exports util functions to clean up node_modules

## Install

```
pnpm install @pnpm/installing.linking.modules-cleaner
```

## API

### `prune(...args)`

Compares the wanted lockfile with the current one and removes redundant packages from `node_modules`.

### `removeDirectDependency(...args)`

Removes a direct dependency from `node_modules`.

## License

MIT
