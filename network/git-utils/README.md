# @pnpm/network.git-utils

> Utilities for git

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/network.git-utils.svg)](https://www.npmjs.com/package/@pnpm/network.git-utils)
<!--/@-->

## Installation

```
pnpm add @pnpm/network.git-utils
```

## Usage

<!--@example('./example.js')-->
```js
'use strict'
const { getCurrentBranchName } = require('@pnpm-utils').default

main()
async function main() {
  const branchName = await getCurrentBranch();
  console.log(branchName)
}
```
<!--/@-->

# License

MIT