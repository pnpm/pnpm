# @pnpm/cmd-shim

> Used in pnpm for command line application support

The cmd-shim used in [pnpm](https://github.com/pnpm/pnpm) to create executable scripts.

## Installation

```sh
npm install --save @pnpm/cmd-shim
```

## API

### `cmdShim(src, to, opts?): Promise<void>`

Create a cmd shim at `to` for the command line program at `src`.
e.g.

```javascript
import { cmdShim } from '@pnpm/cmd-shim'
cmdShim('/path/to/cli.js', '/usr/bin/command-name')
  .catch(err => console.error(err))
```

### `cmdShimIfExists(src, to, opts?): Promise<void>`

The same as above, but will just continue if the file does not exist.

#### Arguments:

- `opts.preserveSymlinks` - _Boolean_ - if true, `--preserve-symlinks` is added to the options passed to NodeJS.
- `opts.nodePath` - _String_ - sets the [NODE_PATH](https://nodejs.org/api/cli.html#cli_node_path_path) env variable.
- `opts.prependToPath` - _String_ - prepends the passed path to PATH before executing the Node.js program.
- `opts.nodeExecPath` - _String_ - sets the path to the Node.js executable.
- `opts.createCmdFile` - _Boolean_ - is `true` on Windows by default. If true, creates a cmd file.
- `opts.createPwshFile` - _Boolean_ - is `true` by default. If true, creates a powershell file.
- `opts.progArgs` - String - optional arguments that will be prepend to any CLI arguments

```javascript
import { cmdShim } from '@pnpm/cmd-shim'
cmdShim('/path/to/cli.js', '/usr/bin/command-name', { preserveSymlinks: true })
  .catch(err => console.error(err))
```

## License

[BSD-2-Clause](./LICENSE) © [Zoltan Kochan](http://kochan.io)
