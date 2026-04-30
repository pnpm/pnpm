# @pnpm/config.reader

> Gets configuration options for pnpm

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/config.reader.svg)](https://www.npmjs.com/package/@pnpm/config.reader)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/config.reader
```

## Usage

```ts
import { getConfig } from '@pnpm/config.reader'

getConfig().then(pnpmConfig => console.log(pnpmConfig))
```

## License

MIT
