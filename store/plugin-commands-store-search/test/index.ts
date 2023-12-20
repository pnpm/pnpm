/// <reference path="../../../__typings__/index.d.ts" />
import path from 'path'

import { prepare } from '@pnpm/prepare'
import { getConfig } from '@pnpm/config'
import { catIndex, catFile } from '@pnpm/plugin-commands-store-search'
import { type PnpmError } from '@pnpm/error'

import execa from 'execa'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

// cat-index
test('print cat index file content', async () => {
  prepare({
    dependencies: {
      bytes: '3.1.2',
    },
  })

  await execa('node', [pnpmBin, 'install'])

  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '8.12.1',
      },
    })
    const output = await catIndex.handler(config as catIndex.catIndexCommandOptions, ['bytes@3.1.2'])

    expect(output).toBe(
      `{
  "name": "bytes",
  "version": "3.1.2",
  "files": {
    "LICENSE": {
      "checkedAt": 1702895929518,
      "integrity": "sha512-lArkAdzOzuKSu1mXyX6LAt35DCs+nqiO9xLMrwBR9k5ioyHImUcuLrEzq0ZDqv3LMPK9sTEyKdW19SkAgn6Y0w==",
      "mode": 420,
      "size": 1153
    },
    "index.js": {
      "checkedAt": 1702895929518,
      "integrity": "sha512-8GMBUqJHz1HfxnfCIyOvzMZnNQoR2wk7Wbk/QDSB3rodRM14zVP0xKPi3yl8Nf5UzchBwQxGZ+u4HTpU+/VtQw==",
      "mode": 420,
      "size": 3613
    },
    "package.json": {
      "checkedAt": 1702895929519,
      "integrity": "sha512-S02JMX4aHKrmkk8jS3XhW9L4vQJtMWFS5s8/+sU1U76imVB2qKNl8mqWcw82Fw0RWsNarm0IiPYh9TbXlbiaLQ==",
      "mode": 420,
      "size": 959
    },
    "History.md": {
      "checkedAt": 1702895929521,
      "integrity": "sha512-Qqv/OrJwwnXelc7sBh1cFuXCdUpLm26/pqNBfdjDFM5JLIbNV0e0Y8YzpFYM2d56xalAh6T4ZOquPyDc1vzk7g==",
      "mode": 420,
      "size": 1775
    },
    "Readme.md": {
      "checkedAt": 1702895929523,
      "integrity": "sha512-ZF9Q82J2KaXGIXWxGC0iecrQmfa08Cs/D+e2BPzGSnOn5aCyWiJbwlMm3HqKMK6qKrBg+/u6LduS/a1mc8IsNQ==",
      "mode": 420,
      "size": 4770
    }
  }
}`)
  }
})

test('prints index file error', async () => {
  let err!: PnpmError
  try {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '8.12.1',
      },
    })
    await catIndex.handler(config as catIndex.catIndexCommandOptions, ['bytes@3.1.1'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_INVALID_PACKAGE')
  expect(err.message).toBe('No corresponding index file found. You can use pnpm list to see if the package is installed.')
})

// cat-file
test('print hash file content', async () => {
  prepare({
    dependencies: {
      bytes: '3.1.2',
    },
  })

  await execa('node', [pnpmBin, 'install'])

  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '8.12.1',
      },
    })
    const output = await catFile.handler(config as catFile.catFileCommandOptions, ['sha512-ZF9Q82J2KaXGIXWxGC0iecrQmfa08Cs/D+e2BPzGSnOn5aCyWiJbwlMm3HqKMK6qKrBg+/u6LduS/a1mc8IsNQ=='])

    expect(output).toBe(`# Bytes utility

[![NPM Version][npm-image]][npm-url]
[![NPM Downloads][downloads-image]][downloads-url]
[![Build Status][ci-image]][ci-url]
[![Test Coverage][coveralls-image]][coveralls-url]

Utility to parse a string bytes (ex: \`1TB\`) to bytes (\`1099511627776\`) and vice-versa.

## Installation

This is a [Node.js](https://nodejs.org/en/) module available through the
[npm registry](https://www.npmjs.com/). Installation is done using the
[\`npm install\` command](https://docs.npmjs.com/getting-started/installing-npm-packages-locally):

\`\`\`bash
$ npm install bytes
\`\`\`

## Usage

\`\`\`js
var bytes = require('bytes');
\`\`\`

#### bytes(number｜string value, [options]): number｜string｜null

Default export function. Delegates to either \`bytes.format\` or \`bytes.parse\` based on the type of \`value\`.

**Arguments**

| Name    | Type     | Description        |
|---------|----------|--------------------|
| value   | \`number\`｜\`string\` | Number value to format or string value to parse |
| options | \`Object\` | Conversion options for \`format\` |

**Returns**

| Name    | Type             | Description                                     |
|---------|------------------|-------------------------------------------------|
| results | \`string\`｜\`number\`｜\`null\` | Return null upon error. Numeric value in bytes, or string value otherwise. |

**Example**

\`\`\`js
bytes(1024);
// output: '1KB'

bytes('1KB');
// output: 1024
\`\`\`

#### bytes.format(number value, [options]): string｜null

Format the given value in bytes into a string. If the value is negative, it is kept as such. If it is a float, it is
 rounded.

**Arguments**

| Name    | Type     | Description        |
|---------|----------|--------------------|
| value   | \`number\` | Value in bytes     |
| options | \`Object\` | Conversion options |

**Options**

| Property          | Type   | Description                                                                             |
|-------------------|--------|-----------------------------------------------------------------------------------------|
| decimalPlaces | \`number\`｜\`null\` | Maximum number of decimal places to include in output. Default value to \`2\`. |
| fixedDecimals | \`boolean\`｜\`null\` | Whether to always display the maximum number of decimal places. Default value to \`false\` |
| thousandsSeparator | \`string\`｜\`null\` | Example of values: \`' '\`, \`','\` and \`'.'\`... Default value to \`''\`. |
| unit | \`string\`｜\`null\` | The unit in which the result will be returned (B/KB/MB/GB/TB). Default value to \`''\` (which means auto detect). |
| unitSeparator | \`string\`｜\`null\` | Separator to use between number and unit. Default value to \`''\`. |

**Returns**

| Name    | Type             | Description                                     |
|---------|------------------|-------------------------------------------------|
| results | \`string\`｜\`null\` | Return null upon error. String value otherwise. |

**Example**

\`\`\`js
bytes.format(1024);
// output: '1KB'

bytes.format(1000);
// output: '1000B'

bytes.format(1000, {thousandsSeparator: ' '});
// output: '1 000B'

bytes.format(1024 * 1.7, {decimalPlaces: 0});
// output: '2KB'

bytes.format(1024, {unitSeparator: ' '});
// output: '1 KB'
\`\`\`

#### bytes.parse(string｜number value): number｜null

Parse the string value into an integer in bytes. If no unit is given, or \`value\`
is a number, it is assumed the value is in bytes.

Supported units and abbreviations are as follows and are case-insensitive:

  * \`b\` for bytes
  * \`kb\` for kilobytes
  * \`mb\` for megabytes
  * \`gb\` for gigabytes
  * \`tb\` for terabytes
  * \`pb\` for petabytes

The units are in powers of two, not ten. This means 1kb = 1024b according to this parser.

**Arguments**

| Name          | Type   | Description        |
|---------------|--------|--------------------|
| value   | \`string\`｜\`number\` | String to parse, or number in bytes.   |

**Returns**

| Name    | Type        | Description             |
|---------|-------------|-------------------------|
| results | \`number\`｜\`null\` | Return null upon error. Value in bytes otherwise. |

**Example**

\`\`\`js
bytes.parse('1KB');
// output: 1024

bytes.parse('1024');
// output: 1024

bytes.parse(1024);
// output: 1024
\`\`\`

## License

[MIT](LICENSE)

[ci-image]: https://badgen.net/github/checks/visionmedia/bytes.js/master?label=ci
[ci-url]: https://github.com/visionmedia/bytes.js/actions?query=workflow%3Aci
[coveralls-image]: https://badgen.net/coveralls/c/github/visionmedia/bytes.js/master
[coveralls-url]: https://coveralls.io/r/visionmedia/bytes.js?branch=master
[downloads-image]: https://badgen.net/npm/dm/bytes
[downloads-url]: https://npmjs.org/package/bytes
[npm-image]: https://badgen.net/npm/v/bytes
[npm-url]: https://npmjs.org/package/bytes
`)
  }
})

test('print hash file content error', async () => {
  let err!: PnpmError
  try {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '8.12.1',
      },
    })
    await catFile.handler(config as catFile.catFileCommandOptions, ['sha512-ZF9Q82J2KaXGIXWxGC0iecrQmfa08'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_INVALID_HASH')
  expect(err.message).toBe('Corresponding hash file not found')
})