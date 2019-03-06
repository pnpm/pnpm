# @pnpm/lockfile-file

> Read/write pnpm-lock.yaml files

Reads and writes the wanted (`pnpm-lock.yaml`) and current (`node_modules/.pnpm-lock.yaml`) lockfile files of pnpm.
Shrinkwrap files are the state files of the `node_modules` installed via pnpm. They are like
the `package-lock.json` of npm or the `yarn.lock` of Yarn.

## Install

```
npm i @pnpm/lockfile-file
```

## API

### `readWanted(pkgPath, opts) => Promise<Shrinkwrap>`

Alias: `read`

Reads the `pnpm-lock.yaml` file from the root of the package.

#### Arguments

* `pkgPath` - *Path* - the path to the project
* `opts.ignoreIncompatible` - *Boolean* - `false` by default. If `true`, throws an error
if the lockfile file format is not compatible with the current library.

### `readCurrent(pkgPath, opts) => Promise<Shrinkwrap>`

Alias: `readPrivate`

Reads the lockfile file from `node_modules/.pnpm-lock.yaml`.

### `existsWanted(pkgPath) => Promise<Boolean>`

Returns `true` if a `pnpm-lock.yaml` exists in the root of the package.

### `write(pkgPath, wantedShrinkwrap, currentShrinkwrap) => Promise<void>`

Writes the wanted/current lockfile files. When they are empty, removes them.

### `writeWantedOnly(pkgPath, wantedShrinkwrap) => Promise<void>`

Writes the wanted lockfile file only. Sometimes it is needed just to update the wanted lockfile
without touching `node_modules`.

### `writeCurrentOnly(pkgPath, currentShrinkwrap) => Promise<void>`

Writes the current lockfile file only. Fails if there is no `node_modules` directory in the `pkgPath`.

## License

[MIT](LICENSE)
