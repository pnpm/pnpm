# @pnpm/lockfile-to-pnp

> Creates a Plug'n'Play file from a pnpm-lock.yaml

## Installation

```
pnpm add -g @pnpm/lockfile-to-pnp
```

## Usage

1. Run `pnpm install` to create an up-to-date lockfile and virtual store.
1. Run `lockfile-to-pnp` in the directory that has a lockfile.
1. When executing a Node script, use the generated `.pnp.js` file to hook Node's resolver.

   E.g., if to run `index.js`, use this command: `node --require ./.pnp.js index.js`

## License 

MIT
