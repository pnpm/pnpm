#!/usr/bin/env node
const [major, minor] = process.version.slice(1).split('.')
const COMPATIBILITY_PAGE = `Visit https://r.pnpm.io/comp to see the list of past pnpm versions with respective Node.js version support.`

if (major < 14 || major == 14 && minor < 19) {
  console.log(`ERROR: This version of pnpm requires at least Node.js v14.19
The current version of Node.js is ${process.version}
${COMPATIBILITY_PAGE}`)
  process.exit(1)
}

require('../dist/pnpm.cjs')

// if you want to debug at your local env, you can use this
// require('../lib/pnpm')
