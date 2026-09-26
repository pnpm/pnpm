# @pnpm/exec.npm-lifecycle

> JavaScript package lifecycle hook runner

`@pnpm/exec.npm-lifecycle` is a library for executing packages' lifecycle
scripts. It is extracted from npm itself and intended to be fully compatible
with the way npm executes individual scripts.

## Installation

```sh
pnpm add @pnpm/exec.npm-lifecycle
```

## API

### `lifecycle(pkg, stage, wd, opts): Promise<void>`

Runs the `stage` script of `pkg` in `wd`, followed by the `.hooks/<stage>`
hook of `opts.dir` when there is one.

* `opts.dir` - the `node_modules` directory whose `.hooks` are run.
* `opts.log` - the logger the runner reports to.
* `opts.stdio` - the [stdio](https://nodejs.org/api/child_process.html#child_process_options_stdio)
passed to the child process. `[0, 1, 2]` by default.
* `opts.runConcurrently` - *Boolean* - `false` by default. If `true`, lifecycle scripts may run concurrently.
* `opts.extraEnv` - *Record<string, string>* - add some extra env vars to the exec environment of the lifecycle script.
* `opts.extraBinPaths` - *string[]* - directories added to the `PATH` of the lifecycle script.
* `opts.onSpawn` - *Function* - called with each spawned lifecycle child process.
* `opts.scriptShell` - the shell the script runs in. `sh` by default, `cmd` on Windows.
* `opts.shellEmulator` - *Boolean* - when `scriptShell` is unset, run the script in a JavaScript shell emulator instead of a system shell. A configured `scriptShell` is used even when this is set.
* `opts.scriptsPrependNodePath` - *Boolean | 'warn-only'* - put the directory of the running Node.js binary on the script's `PATH`.

### `makeEnv(pkg, opts): Record<string, string>`

Builds the environment a lifecycle script of `pkg` runs with: the current
environment without private `npm_config_*` settings, plus the `npm_package_*`
variables derived from the manifest.

## License

[Artistic-2.0](./LICENSE)
