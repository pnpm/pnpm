# @pnpm/git-utils

> Utilities for git

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/git-utils.svg)](https://www.npmjs.com/package/@pnpm/git-utils)
<!--/@-->

## Installation

```
pnpm add @pnpm/git-utils
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