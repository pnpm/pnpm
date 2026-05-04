# @pnpm/modules-mounter.daemon

> Mounts a node_modules directory with FUSE

[![npm version](https://img.shields.io/npm/v/@pnpm/modules-mounter.daemon.svg)](https://npmx.dev/package/@pnpm/modules-mounter.daemon)

## Installation

```
pnpm add @pnpm/modules-mounter.daemon --global
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
