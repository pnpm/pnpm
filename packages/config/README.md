# @pnpm/config

> Gets configs for pnpm

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/config.svg)](https://www.npmjs.com/package/@pnpm/config) [![Build Status](https://img.shields.io/travis/pnpm/config/master.svg)](https://travis-ci.org/pnpm/config)
<!--/@-->

## Installation

```sh
npm i -S @pnpm/config
```

## Usage

```ts
import getConfigs from '@pnpm/config'

getConfigs().then(pnpmConfigs => console.log(pnpmConfigs))
```

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
