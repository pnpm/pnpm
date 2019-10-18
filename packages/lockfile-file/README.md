# @pnpm/lockfile-file

> Read/write pnpm-lock.yaml files

Reads and writes the wanted (`pnpm-lock.yaml`) and current (`node_modules/.pnpm-lock.yaml`) lockfile files of pnpm.
Lockfile files are the state files of the `node_modules` installed via pnpm. They are like
the `package-lock.json` of npm or the `yarn.lock` of Yarn.

## Install

```
<pnpm|yarn|npm> add @pnpm/lockfile-file
```

## API

### `readWantedLockfile(pkgPath, opts) => Promise<Lockfile>`

Reads the `pnpm-lock.yaml` file from the root of the package.

#### Arguments

* `pkgPath` - *Path* - the path to the project
* `opts.ignoreIncompatible` - *Boolean* - `false` by default. If `true`, throws an error
if the lockfile file format is not compatible with the current library.

### `readCurrentLockfile(virtualStoreDir, opts) => Promise<Lockfile>`

Reads the lockfile file from `<virtualStoreDir>/lock.yaml`.

### `existsWantedLockfile(pkgPath) => Promise<Boolean>`

Returns `true` if a `pnpm-lock.yaml` exists in the root of the package.

### `writeLockfiles(opts) => Promise<void>`

Writes the wanted/current lockfile files. When they are empty, removes them.

#### Arguments

* `opts.wantedLockfile`
* `opts.wantedLockfileDir`
* `opts.currentLockfile`
* `opts.currentLockfileDir`
* `[opts.forceSharedFormat]`

### `writeWantedLockfile(pkgPath, wantedLockfile) => Promise<void>`

Writes the wanted lockfile file only. Sometimes it is needed just to update the wanted lockfile
without touching `node_modules`.

### `writeCurrentLockfile(virtualStoreDir, currentLockfile) => Promise<void>`

Writes the current lockfile file only.

## License

[MIT](LICENSE)
