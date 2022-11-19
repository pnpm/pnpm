# @pnpm/store-path

> Resolves the pnpm store path

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/store-path.svg)](https://www.npmjs.com/package/@pnpm/store-path) [![Build Status](https://img.shields.io/travis/pnpm/store-path/master.svg)](https://travis-ci.org/pnpm/store-path)
<!--/@-->

## Installation

```sh
<pnpm|yarn|npm> add @pnpm/store-path
```

## Usage

```ts
import resolveStorePath from '@pnpm/store-path'

await resolveStorePath('F:\\project', 'pnpm-store')
//> F:\\pnpm-store\\2
```

## License

[MIT](./LICENSE)
