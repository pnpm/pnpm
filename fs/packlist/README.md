# @pnpm/fs.packlist

> Get a list of the files to add from a directory into an npm package

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/packlist.svg)](https://npmx.dev/package/@pnpm/fs.packlist)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/fs.packlist
```

## Usage

```js
const { packlist } = require('@pnpm/fs.packlist')

const files = packlist('/package-dir')
```

## License

MIT © [Zoltan Kochan](https://www.kochan.io)
