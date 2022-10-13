# @pnpm/error

> An error class for pnpm errors

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/error.svg)](https://www.npmjs.com/package/@pnpm/error)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/error
```

### Usage

```ts
import { PnpmError } from '@pnpm/error'

try {
    throw new PnpmError('THE_ERROR_CODE', 'The error message')
} catch (err: any) { // eslint-disable-line
    console.log(err.code)
    //> ERR_PNPM_THE_ERROR_CODE
    console.log(err.message)
    //> The error message
}
```

## License

MIT
