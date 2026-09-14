# @pnpm/bins.cmd-shim

> Used in pnpm for command line application support

The cmd-shim used in [pnpm](https://github.com/pnpm/pnpm) to create executable scripts.

## Installation

```sh
npm install --save @pnpm/bins.cmd-shim
```

## API

### `cmdShim(src, to, opts?): Promise<void>`

Create a cmd shim at `to` for the command line program at `src`.
e.g.

```javascript
import { cmdShim } from '@pnpm/bins.cmd-shim'
cmdShim('/path/to/cli.js', '/usr/bin/command-name')
  .catch(err => console.error(err))
```

### `cmdShimIfExists(src, to, opts?): Promise<void>`

Creates shims like `cmdShim`, but ignores all creation errors, including a missing source file.

#### Arguments:

- `opts.preserveSymlinks` - _Boolean_ - if true, `--preserve-symlinks` is added to the options passed to NodeJS.
- `opts.nodePath` - `string | string[]` - sets the [NODE_PATH](https://nodejs.org/api/cli.html#cli_node_path_path) env variable.
- `opts.prependToPath` - _String_ - prepends the passed path to PATH before executing the Node.js program.
- `opts.nodeExecPath` - _String_ - sets the path to the Node.js executable.
- `opts.createCmdFile` - _Boolean_ - is `true` on Windows by default. If true, creates a cmd file.
- `opts.createPwshFile` - _Boolean_ - is `true` by default. If true, creates a powershell file.
- `opts.progArgs` - `string[]` - optional arguments prepended to the CLI arguments

```javascript
import { cmdShim } from '@pnpm/bins.cmd-shim'
cmdShim('/path/to/cli.js', '/usr/bin/command-name', { preserveSymlinks: true })
  .catch(err => console.error(err))
```

## License

[BSD-2-Clause](./LICENSE) © [Zoltan Kochan](http://kochan.io)
