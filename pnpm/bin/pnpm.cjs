#!/usr/bin/env node
const [major, minor] = process.version.slice(1).split('.')
const COMPATIBILITY_PAGE = `Visit https://r.pnpm.io/comp to see the list of past pnpm versions with respective Node.js version support.`

// We don't use the semver library here because:
//  1. it is already bundled to dist/pnpm.cjs, so we would load it twice
//  2. we want this file to support potentially older Node.js versions than what semver supports
if (major < 16 || major == 16 && minor < 14) {
  console.log(`ERROR: This version of pnpm requires at least Node.js v16.14
The current version of Node.js is ${process.version}
${COMPATIBILITY_PAGE}`)
  process.exit(1)
}

global['pnpm__startedAt'] = Date.now()
require('../dist/pnpm.cjs')

// if you want to debug at your local env, you can use this
// require('../lib/pnpm')
