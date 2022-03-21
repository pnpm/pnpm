# @pnpm/ts-execution-runtime

> pnpm's TypeScript execution runtime

## Usage

Create the `js` file in the TypeScript package you want to execute directly from source with the following contents:

```js
require('@pnpm/ts-execution-runtime')

require('./src/index.ts')
```

## Rationale

There are cases when the contributor wants to check changes to `pnpm` codebase as quick as possible. The TypeScript compiler does not currently let the user compile the code without typechecking, thus this process is pretty slow. The typechecking step can also be skipped for quick changes, because editors typically have `eslint` integration and do typechecking inside
modified files.

This module allows to use `@babel/register` to transpile `pnpm` TypeScript source code on the fly
without typechecking. In order to use this module on `pnpm` source code, one needs to execute: `node <repo_directory>/packages/pnpm/spnpm [command] [flags]`

## License

MIT
