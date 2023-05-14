# @pnpm/mount-modules

> Mounts a node_modules directory with FUSE

[![npm version](https://img.shields.io/npm/v/@pnpm/mount-modules.svg)](https://www.npmjs.com/package/@pnpm/mount-modules)

## Installation

```
pnpm add @pnpm/mount-modules --global
```

## Usage

Before mounting the modules directory, all the packages should be fetched to the store. This can be done by running:

```
pnpm install --lockfile-only
```

Once the packages are in the store, run:

```
mount-modules
```

If something goes wrong and the modules directory will be not accessible, unmount it using:

```
unmount <path to node_modules>
```

## License

MIT
